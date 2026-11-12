use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use hearthd_config::ConfigValue;
use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, ConfigValue)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    S2000,
}

#[derive(Debug, Clone, Deserialize, Serialize, ConfigValue)]
#[serde(try_from = "String")]
pub struct AdapterName(String);

impl TryFrom<String> for AdapterName {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let suffix = value.strip_prefix("hci").unwrap_or("");
        if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("adapter must name a BlueZ adapter such as hci0");
        }
        Ok(Self(value))
    }
}

impl FromStr for AdapterName {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl Deref for AdapterName {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize, ConfigValue)]
#[serde(try_from = "String")]
pub struct Serial(String);

impl TryFrom<String> for Serial {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 32
            || !value.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err("serial must contain 1 to 32 ASCII letters or digits");
        }
        Ok(Self(value))
    }
}

impl FromStr for Serial {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl Deref for Serial {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Serial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceName(String);

impl FromStr for DeviceName {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() {
            return Err("device name must not be empty");
        }
        Ok(Self(value.to_owned()))
    }
}

impl Deref for DeviceName {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, ConfigValue)]
#[serde(try_from = "String", into = "String")]
pub struct BluetoothAddress([u8; 6]);

impl BluetoothAddress {
    pub fn from_octets(octets: [u8; 6]) -> Self {
        Self(octets)
    }

    pub fn octets(self) -> [u8; 6] {
        self.0
    }
}

impl FromStr for BluetoothAddress {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        const ERROR: &str = "address must be six colon-separated hexadecimal octets";
        let parts: Vec<_> = value.split(':').collect();
        if parts.len() != 6
            || parts
                .iter()
                .any(|part| part.len() != 2 || !part.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(ERROR);
        }
        let mut octets = [0; 6];
        for (octet, part) in octets.iter_mut().zip(parts) {
            *octet = u8::from_str_radix(part, 16).map_err(|_| ERROR)?;
        }
        Ok(Self(octets))
    }
}

impl TryFrom<String> for BluetoothAddress {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<BluetoothAddress> for String {
    fn from(value: BluetoothAddress) -> Self {
        value.to_string()
    }
}

impl fmt::Display for BluetoothAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02X}:{b:02X}:{c:02X}:{d:02X}:{e:02X}:{g:02X}")
    }
}

/// A persistent protocol identity, serialized as the UUID's canonical text.
#[derive(Clone, Copy, Deserialize, Serialize, ConfigValue)]
#[serde(try_from = "Uuid")]
pub struct ClientId(Uuid);

impl ClientId {
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl TryFrom<Uuid> for ClientId {
    type Error = &'static str;

    fn try_from(value: Uuid) -> Result<Self, Self::Error> {
        if value.is_nil() {
            Err("client_id must be a nonzero UUID")
        } else {
            Ok(Self(value))
        }
    }
}

impl FromStr for ClientId {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value)
            .map_err(|_| "client_id must be a UUID")?
            .try_into()
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.hyphenated(), f)
    }
}
