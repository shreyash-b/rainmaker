use components::{persistent_storage::NvsPartition, wifi_prov::WiFiProvMgrBle};
use esp_idf_svc::hal::gpio::InputPin;
use gpi_driver::GPIDriver;

pub mod gpi_driver;

const PROV_RESET_PRESS_DELAY: u32 = 3000;

pub fn reg_reset_trigger<T>(btn: &mut GPIDriver<T>, nvs_parition: NvsPartition)
where
    T: InputPin,
{
    btn.set_press_cb(
        Box::new(move || {
            WiFiProvMgrBle::reset_provisioning(nvs_parition.clone())
                .expect("Failed to reset WiFi Provisioning ")
        }),
        PROV_RESET_PRESS_DELAY,
        "resetting WiFi Provisioning",
    )
}
