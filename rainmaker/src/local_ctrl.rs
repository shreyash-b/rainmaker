use std::{collections::HashMap, process::Command, sync::Arc};

use rainmaker_components::local_ctrl::LocalControl;
use serde_json::Value;

use crate::node::Node;

const LOCAL_CTRL_TYPE_NODECONFIG: u32 = 1;
const LOCAL_CTRL_TYPE_PARAM: u32 = 2;

const LOCAL_CTRL_FLAG_READONLY: u32 = 1;

pub struct RmakerLocalCtrl {
    local_ctrl: LocalControl,
}

impl RmakerLocalCtrl {
    pub fn new(node: Arc<Node>, node_id: &str) -> RmakerLocalCtrl {
        let node_2 = node.clone();
        let mut local_ctrl = LocalControl::new(
            Box::new(move |name, type_, flags| local_ctrl_get_val(name, type_, flags, &node)),
            Box::new(move |name, type_, flags, data| {
                local_ctrl_set_val(name, type_, flags, data, &node_2)
            }),
        );
        local_ctrl.add_property(
            "config".to_string(),
            LOCAL_CTRL_TYPE_NODECONFIG,
            LOCAL_CTRL_FLAG_READONLY,
        );
        local_ctrl.add_property("params".to_string(), LOCAL_CTRL_TYPE_PARAM, 0);

        #[cfg(target_os = "linux")]
        {
            advertise_mdns(node_id);
            RmakerLocalCtrl { local_ctrl }
        }

        #[cfg(target_os = "espidf")]
        {
            RmakerLocalCtrl { local_ctrl }
        }
    }
}

#[cfg(target_os = "linux")]
fn advertise_mdns(node_id: &str) {
    let mut command = Command::new("avahi-publish");
    command.args([
        "--service",
        &format!("{}", node_id),
        "_esp_local_ctrl._tcp",
        "8080",
        "version_endpoint=\"/esp_local_ctrl/version\"",
        "session_endpoint=\"/esp_local_ctrl/session\"",
        "control_endpoint=\"/esp_local_ctrl/control\"",
        &format!("node_id={}", node_id),
    ]);

    // TODO: check if avertisement is properly started and store this child handle somewhere
    let _child = command.spawn();
}

fn local_ctrl_get_val(name: &str, _prop_type: u32, _flags: u32, node: &Arc<Node>) -> Vec<u8> {
    let res = match name {
        "config" => serde_json::to_vec(node.as_ref()).unwrap(),
        "params" => {
            let params = node.get_param_values();
            serde_json::to_vec(&params).unwrap()
        }
        _ => {
            log::error!("Trying to set unknown proprty {}", name);
            return Default::default();
        }
    };

    res
}

fn local_ctrl_set_val(name: &str, _prop_type: u32, flags: u32, data: Vec<u8>, node: &Arc<Node>) {
    if flags == LOCAL_CTRL_FLAG_READONLY {
        log::error!("Trying to modify read only property");
        return;
    }
    match name {
        "params" => {
            // TODO: Make appropriate changes to use &str instead of String for parameter name
            let params: HashMap<&str, HashMap<String, Value>> =
                serde_json::from_slice(&data).unwrap();
            for (device, params) in params {
                node.exeute_device_callback(device, params);
            }
        }
        _ => {
            log::error!("Trying to set unknown property: {}", name);
        }
    }
}
