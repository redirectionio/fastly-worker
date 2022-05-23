use std::string::FromUtf8Error;

quick_error! {
    #[derive(Debug)]
    pub enum InternalError {
        EncodingNotSupported {
            display("Encoding not compressed")
        }
        DecodingFailed (e: String) {
            display("Decoding response failed")
        }
    }
}

impl From<std::io::Error> for InternalError {
    fn from(e: std::io::Error) -> Self {
        InternalError::DecodingFailed(e.to_string())
    }
}

impl From<FromUtf8Error> for InternalError {
    fn from(e: FromUtf8Error) -> Self {
        InternalError::DecodingFailed(e.to_string())
    }
}
