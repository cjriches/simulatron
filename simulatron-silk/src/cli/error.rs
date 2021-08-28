use simulatron_silk::OFError;

pub struct LinkError(pub String);

impl From<OFError> for LinkError {
    fn from(e: OFError) -> Self {
        LinkError(e.message().to_string())
    }
}
