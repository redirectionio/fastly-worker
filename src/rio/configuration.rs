#[readonly::make]
pub struct Configuration {
    pub backend_name: String,
    pub token: String,
    pub instance_name: String,
    pub add_rule_ids_header: bool,
}

impl Configuration {
    pub(crate) fn new(
        backend_name: Option<String>,
        token: Option<String>,
        instance_name: Option<String>,
        add_rule_ids_header: Option<String>,
    ) -> Result<Self, ConfigurationError> {
        let backend_name = match backend_name {
            Some(backend_name) => backend_name,
            None => return Err(ConfigurationError::MissingBackendName),
        };

        let token = match token {
            Some(token) => token,
            None => return Err(ConfigurationError::MissingToken(backend_name)),
        };

        let instance_name = match instance_name {
            Some(instance_name) => instance_name,
            None => return Err(ConfigurationError::MissingInstanceName(backend_name)),
        };

        let add_rule_ids_header = match add_rule_ids_header {
            Some(add_rule_ids_header) => add_rule_ids_header == "true",
            None => false,
        };

        Ok(Configuration {
            backend_name,
            token,
            instance_name,
            add_rule_ids_header,
        })
    }
}

quick_error! {
    #[derive(Debug)]
    pub enum ConfigurationError {
        MissingBackendName {
            display("missing \"backend_name\"")
        }
        MissingToken (backend_name: String) {
            display("missing \"token\"")
        }
        MissingInstanceName (backend_name: String) {
            display("missing \"instance name\"")
        }
        MissingAddRuleIdsHeader (backend_name: String) {
            display("missing \"add_rule_ids_header\"")
        }
    }
}
