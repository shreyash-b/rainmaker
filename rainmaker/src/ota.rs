#![cfg(target_os = "espidf")]
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::Ok;
use embedded_svc::http::{client::Client as HttpClient, Headers};
use esp_idf_svc::{
    http::{
        client::{Configuration, EspHttpConnection},
        Method as HttpMethod,
    },
    ota::{EspOta, SlotState},
};
use rainmaker_components::{
    mqtt::ReceivedMessage,
    persistent_storage::{Nvs, NvsPartition},
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::{rmaker_mqtt, OTASTATUS_TOPIC_SUFFIX};

const OTA_ROLLBACK_CHECK_DURATION: u64 = 10000; // millis
const HTTPS_OTA_BUFFER_LEN: usize = 2048; // bytes

#[derive(Serialize, Debug)]
pub enum OtaSatus {
    #[serde(rename = "in-progress")]
    InProgress,
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "rejected")]
    Rejected,
}

pub struct RmakerOta {
    node_id: String,
    ota_in_progress: Arc<Mutex<bool>>,
    nvs_partition: NvsPartition,
}

impl RmakerOta {
    pub fn new(node_id: String, nvs_partition: NvsPartition) -> anyhow::Result<Self> {
        let in_progress = Arc::new(Mutex::new(false));
        Ok(Self {
            node_id,
            ota_in_progress: in_progress,
            nvs_partition,
        })
    }

    pub fn apply_ota(&self, ota_job_id: &str, url: &str) -> anyhow::Result<()> {
        let in_progress = self.ota_in_progress.lock().unwrap();
        if *in_progress == true {
            log::warn!("OTA already in progress");
            return Ok(());
        }

        let conn = EspHttpConnection::new(&Configuration {
            buffer_size: Some(1536),
            buffer_size_tx: Some(1536),
            ..Default::default()
        })?;
        let mut client = HttpClient::wrap(conn);

        let request = client.request(HttpMethod::Get, url, &[])?;
        let mut response = request.submit()?;

        let image_len = match response.content_len() {
            Some(len) => {
                log::info!("Image length = {}", len);
                len as u32
            }
            None => unreachable!(),
        };

        let node_id = &self.node_id;

        let mut total_read_len = 0u32;
        let mut buff = [0; HTTPS_OTA_BUFFER_LEN];

        Self::report_status(
            node_id,
            ota_job_id,
            OtaSatus::InProgress,
            "Starting OTA download",
        );
        let mut ota = EspOta::new()?;
        let mut ota_update = ota.initiate_update()?;

        loop {
            let read_len = response.read(&mut buff)?;
            if read_len == 0 {
                continue;
            }

            if total_read_len % 10240 == 0 {
                log::info!("Read {} bytes out of {}", total_read_len, image_len);
            }

            ota_update.write(&buff[..read_len])?;

            total_read_len += read_len as u32;

            if total_read_len == image_len {
                break;
            }
        }

        log::info!("OTA download complete");
        ota_update.complete()?;

        log::info!("Saving OTA job id");
        let mut nvs = Nvs::new(self.nvs_partition.clone(), "rmaker_ota")?;
        nvs.set_bytes("ota_job_id", ota_job_id.as_bytes())?;

        Self::report_status(
            &node_id,
            ota_job_id,
            OtaSatus::InProgress,
            "Download Complete. Rebooting to new firmware",
        );

        log::info!("Rebooting to new firmware in 10 seconds");
        esp_idf_svc::hal::delay::Delay::new_default().delay_ms(10000);

        esp_idf_svc::hal::reset::restart();
    }

    pub fn manage_rollback(&self) -> anyhow::Result<()> {
        let ota = EspOta::new()?;

        let nvs = Nvs::new(self.nvs_partition.clone(), "rmaker_ota")?;
        let mut buff = [0u8; 64];
        match nvs.get_bytes("ota_job_id", &mut buff)? {
            Some(ota_job_id) => {
                let ota_job_id = String::from_utf8(ota_job_id).unwrap();
                let ota_in_progress = self.ota_in_progress.clone();
                let mut in_progress = ota_in_progress.lock().unwrap();
                *in_progress = true;
                drop(in_progress);

                self.verify_ota(ota, ota_in_progress, ota_job_id, nvs)?;
            }
            None => {}
        }

        Ok(())
    }

    fn verify_ota(
        &self,
        ota: EspOta,
        ota_in_progress: Arc<Mutex<bool>>,
        ota_job_id: String,
        mut nvs: Nvs,
    ) -> anyhow::Result<()> {
        let node_id = self.node_id.clone();
        match ota.get_running_slot()?.state {
            SlotState::Valid => {
                RmakerOta::report_status(
                    &node_id,
                    &ota_job_id,
                    OtaSatus::Rejected,
                    "Firmware rolled back",
                );
                let mut in_progress = ota_in_progress.lock().unwrap();
                *in_progress = false;
                nvs.remove("ota_job_id")?;
            }
            SlotState::Unverified => {
                thread::spawn(move || RmakerOta::validate_ota(node_id, ota, ota_job_id, nvs));
            }
            other => {
                log::warn!("Firmware State: {:?}. Not doing anything", other);
                let mut in_progress = ota_in_progress.lock().unwrap();
                *in_progress = false;
                nvs.remove("ota_job_id")?;
            }
        };

        Ok(())
    }

    fn validate_ota(node_id: String, mut ota: EspOta, ota_job_id: String, mut nvs: Nvs) {
        // wait for 1.5 mins and check MQTT connectivity
        thread::sleep(Duration::from_millis(OTA_ROLLBACK_CHECK_DURATION));
        if rmaker_mqtt::is_mqtt_connected() {
            log::warn!("Firmware validated successfully");
            if let Err(e) = ota.mark_running_slot_valid() {
                log::error!("Failure in marking slot as valid: {:?}", e);
            } else {
                RmakerOta::report_status(
                    &node_id,
                    &ota_job_id,
                    OtaSatus::Success,
                    "Firmware verified successfully",
                );

                nvs.remove("ota_job_id").unwrap();
            }
        } else {
            log::error!("Could not validate firmware. Rolling back.");
            thread::sleep(Duration::from_millis(1000));
            ota.mark_running_slot_invalid_and_reboot();
        }
    }

    fn report_status(node_id: &str, ota_job_id: &str, status: OtaSatus, additional_info: &str) {
        let payload = json!({
            "ota_job_id": ota_job_id,
            "status": status,
            "additional_info": additional_info
        });

        let topic = format!("node/{}/{}", node_id, OTASTATUS_TOPIC_SUFFIX);

        log::info!("Reporting {:?} - {}", status, additional_info);
        rmaker_mqtt::publish(&topic, payload.to_string().into_bytes()).unwrap();
    }
}

pub(crate) fn otafetch_callback(msg: ReceivedMessage, ota: &RmakerOta) {
    let ota_info: Value = serde_json::from_str(&String::from_utf8(msg.payload).unwrap()).unwrap();

    #[allow(unused_variables)]
    let ota_url = ota_info
        .as_object()
        .unwrap()
        .get("url")
        .unwrap()
        .as_str()
        .unwrap();

    let ota_job_id = ota_info
        .as_object()
        .unwrap()
        .get("ota_job_id")
        .unwrap()
        .as_str()
        .unwrap();

    if let Err(err) = ota.apply_ota(ota_job_id, ota_url) {
        log::error!("Failed to apply OTA: {:?}", err);
    }
}
