#[macro_use]
extern crate quick_error;

mod rio;

use crate::rio::application::Application;
use crate::rio::configuration::{Configuration, ConfigurationError};
use crate::rio::logging::{Context, FastlyLogger};
use fastly::{Dictionary, Error, Request, Response};

#[fastly::main]
fn main(req: Request) -> Result<Response, Error> {
    let redirection_dict = Dictionary::open("redirectionio");
    let fastly_logger = FastlyLogger::new(
        redirection_dict.get("log_endpoint"),
        redirection_dict.get("log_level"),
        Context::new(req.clone_without_body()),
    );

    let config = match Configuration::new(
        redirection_dict.get("backend_name"),
        redirection_dict.get("token"),
        redirection_dict.get("instance_name"),
        redirection_dict.get("add_rule_ids_header"),
    ) {
        Ok(config) => config,
        Err(error) => {
            return match error {
                ConfigurationError::MissingBackendName => {
                    let message = format!("Fastly worker configuration error: {}.\n", error);
                    fastly_logger.log_error(message.clone(), None);

                    Ok(generate_synthetic_response(message, 500))
                }
                ConfigurationError::MissingToken(ref backend_name)
                | ConfigurationError::MissingInstanceName(ref backend_name)
                | ConfigurationError::MissingAddRuleIdsHeader(ref backend_name) => {
                    // The worked can not be configured: log an error and transparently forward the
                    // request to the backend with no changes
                    let message = format!("Fastly worker configuration error: {}.\n", error);
                    fastly_logger.log_error(message.clone(), None);

                    Ok(req.send(backend_name)?)
                }
            };
        }
    };

    let application = Application::new(&config, &fastly_logger);
    fastly_logger.log_info("Start worker".to_string(), None);

    let rio_request = match application.create_rio_request(&req) {
        Some(rio_request) => rio_request,
        None => return Ok(req.send(config.backend_name.clone())?),
    };

    let mut rio_action = match application.get_action(&rio_request) {
        Some(rio_action) => rio_action,
        None => return Ok(req.send(config.backend_name.clone())?),
    };

    match application.proxy(req, &mut rio_action) {
        Ok(response) => {
            application.log(&response, &rio_request, &mut rio_action);
            Ok(response)
        }
        Err(error) => Err(error),
    }
}

fn generate_synthetic_response(error_message: String, status_code: u16) -> Response {
    let mut response = Response::new();
    response.set_body(error_message);
    response.set_status(status_code);

    return response;
}
