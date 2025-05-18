pub mod serd_u32_hex {
    pub fn serialize<S>(num: &u32, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let hex = format!("{}", hex::encode(num.to_le_bytes()));
        serializer.serialize_str(&hex)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u32, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let hex_str: String = serde::Deserialize::deserialize(deserializer)?;
        u32::from_str_radix(&hex_str, 16).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug)]
pub enum DecodeHeaderError {
    InvalidLength,
    IntParseError,
}

impl std::fmt::Display for DecodeHeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeHeaderError::InvalidLength => write!(f, "Invalid length"),
            DecodeHeaderError::IntParseError => write!(f, "Integer parse error"),
        }
    }
}

impl std::error::Error for DecodeHeaderError {}