use serde::{Deserialize, Serialize};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

#[derive(Serialize, Deserialize)]
pub struct PaginationCursor {
    pub offset: usize,
}

impl PaginationCursor {
    // Encode the cursor struct into a Base64 string
    pub fn encode(&self) -> String {
        let json = serde_json::to_string(self).unwrap();
        URL_SAFE_NO_PAD.encode(json)
    }

    // Decode a Base64 string back into a cursor struct
    pub fn decode(cursor_str: &str) -> Result<Self, ()> {
        let bytes = URL_SAFE_NO_PAD.decode(cursor_str).map_err(|_| ())?;
        serde_json::from_slice(&bytes).map_err(|_| ())
    }
}