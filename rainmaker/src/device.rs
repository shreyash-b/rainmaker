//! Device module.
//!
//! One or more devices can be created and registered with node using device module. Device is a
//! physical entity which associated with a node and can be controlled from ESP-RainMaker
//! Dashboard. One or more devices can be created and registered with node using device module.
//!
//! Example for creating an instance of device:
//! ```rust
//! let device = Device::new(name:"DeviceName", device_type:DeviceType::Switch);
//! ```
//!
//! Parameter from [Param] module can be added as following:
//! ```rust
//! let power_param = Param::new_power(name:"Power", initial_value: false);
//! device.add_param(power_param);
//! device.set_primary_param(param_name: "Power");
//! ```
//!
//! A callback needed to be set for every device in order to report updated values of parameters.
//!
//! Example for device callback:
//! ```rust
//! fn device_callback(params: HashMap<String, Value>, devcie_handle: DeviceHandle){
//!     /* Write code for logging the received and reported values */
//!     device_handle.update_and_report(params); // for reporting that params values were successfully updated
//! }
//! ```
//!
//! The callback created can be associated with device using [register_callback].
//! ```rust
//! device.register_callback(Box::new(device_callback));
//! ```
//!
//! [Param]: crate::param::Param
//! [register_callback]: crate::device::Device::register_callback

use std::{collections::HashMap, fmt::Debug};

use serde::Serialize;
use serde_json::{json, Value};

use crate::{factory, param::Param, rmaker_mqtt, NODE_PARAMS_LOCAL_TOPIC_SUFFIX};

pub(crate) type DeviceCbType =
    Box<dyn for<'a> Fn(HashMap<String, Value>, DeviceHandle<'a>) + Send + Sync + 'static>;

#[derive(Serialize)]
pub struct Device {
    name: String,
    #[serde(rename = "type")]
    device_type: DeviceType,
    #[serde(skip_serializing_if = "Option::is_none", rename = "primary")]
    primary_param: Option<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    attributes: HashMap<String, String>,
    params: Vec<Param>,
    #[serde(skip_serializing)]
    callback: Option<DeviceCbType>,
    #[serde(skip_serializing)]
    local_params_topic: String,
}

pub struct DeviceHandle<'a> {
    pub params: &'a [Param],
    pub name: &'a str,
    local_params_topic: &'a str,
}

impl Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("name", &self.name)
            .field("device_type", &self.device_type)
            .field("primary_param", &self.primary_param)
            .field("attributes", &self.attributes)
            .field("params", &self.params)
            .finish()
    }
}

impl Device {
    /// This function creates an instance of device.
    pub fn new(name: &str, device_type: DeviceType) -> Self {
        let mut buff = [0u8; 32];
        let node_id = factory::get_node_id(&mut buff).unwrap();
        let local_params_topic = format!("node/{}/{}", node_id, NODE_PARAMS_LOCAL_TOPIC_SUFFIX);

        Self {
            name: name.to_owned(),
            device_type,
            primary_param: None,
            attributes: Default::default(),
            params: vec![],
            callback: None,
            local_params_topic,
        }
    }

    /// A parameter can be set as a primary parameter.
    pub fn set_primary_param(&mut self, param_name: &str) {
        self.primary_param = Some(param_name.to_string())
    }

    pub fn add_attribute(&mut self, name: String, value: String) {
        self.attributes
            .insert(name, value)
            .expect("Failed to add attribute");
    }

    /// This function associates a parameter with the device.
    pub fn add_param(&mut self, param: Param) {
        self.params.push(param);
    }

    /// This function associates a callback that reports updates values of parameters.
    pub fn register_callback(&mut self, cb: DeviceCbType) {
        self.callback = Some(Box::new(cb));
    }

    /// Function for assigning a name to device.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// This function associates a list of parameters to the device.
    pub fn params(&self) -> &[Param] {
        &self.params
    }

    pub(crate) fn execute_callback(&self, params: HashMap<String, /* ParamDataType */ Value>) {
        let cb = if self.callback.is_some() {
            self.callback.as_ref().unwrap()
        } else {
            return;
        };

        let handle = DeviceHandle {
            params: &self.params,
            name: &self.name,
            local_params_topic: &self.local_params_topic,
        };

        cb(params, handle);
    }
}

impl DeviceHandle<'_> {
    /// Reports parameters values of devices to the RainMaker cloud over MQTT.
    ///
    /// Appropriate parameters(name: value) must be provided.
    ///
    /// Example (Can be used in a device callback function)
    /// ```
    /// fn device_cb(params: HashMaps<String, Value>, devcie_handle: DeviceHandle)
    /// {
    ///     log::info!("Received update: {:?}", params);
    ///     log::info!("Reporting: {:?}", params);
    ///     devcie_handle.update_and_report(params);
    /// }
    /// ```
    pub fn update_and_report(&self, params: HashMap<String, Value>) {
        let updated_params = json!({
            self.name: params
        });

        rmaker_mqtt::publish(
            self.local_params_topic,
            updated_params.to_string().into_bytes(),
        )
        .unwrap();
    }
}

/// ESP RainMaker provides a set of standard devices. These are provided with a UI and have special handling in clients like Alexa/Google Home.
///
/// Refer [device list](https://rainmaker.espressif.com/docs/standard-types).
#[derive(Debug, Serialize)]
pub enum DeviceType {
    #[serde(rename = "esp.device.switch")]
    Switch,
    #[serde(rename = "esp.device.lightbulb")]
    Lightbulb,
    #[serde(rename = "esp.device.light")]
    Light,
    #[serde(rename = "esp.device.fan")]
    Fan,
    #[serde(rename = "esp.device.temperature-sensor")]
    TemperatureSensor,
    #[serde(rename = "esp.device.outlet")]
    SmartPlugOutlet,
    #[serde(rename = "esp.device.plug")]
    Smartplug,
    #[serde(rename = "esp.device.socket")]
    SmartplugSocket,
    #[serde(rename = "esp.device.lock")]
    Smartlock,
    #[serde(rename = "esp.device.blinds-internal")]
    InteriorBlind,
    #[serde(rename = "esp.device.blinds-external")]
    ExteriorBlind,
    #[serde(rename = "esp.device.garage-door")]
    GarageDoor,
    #[serde(rename = "esp.device.speaker")]
    Speaker,
    #[serde(rename = "esp.device.air-conditioner")]
    AirConditioner,
    #[serde(rename = "esp.device.thermostat")]
    Thermostat,
    #[serde(rename = "esp.device.tv")]
    TV,
    #[serde(rename = "esp.device.washer")]
    Washer,
    #[serde(rename = "esp.device.contact-sensor")]
    ContactSensor,
    #[serde(rename = "esp.device.motion-sensor")]
    MotionSensor,
    #[serde(rename = "esp.device.doorbell")]
    Doorbell,
    #[serde(rename = "esp.device.security-panel")]
    SecurityPanel,
    #[serde(rename = "esp.device.water-heater")]
    X,
    #[serde(rename = "esp.device.other")]
    OTHER,
}
