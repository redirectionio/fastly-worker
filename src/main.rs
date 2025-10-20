#[macro_use]
extern crate quick_error;

mod rio;

use crate::rio::application::Application;
use crate::rio::configuration::{Configuration, ConfigurationError};
use crate::rio::logging::{Context, FastlyLogger};
use crate::rio::request_sender::{DirectRequestSender, RequestSender};
use fastly::{ConfigStore, Error, Request, Response};

#[fastly::main]
fn main(req: Request) -> Result<Response, Error> {
    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|time| time.as_millis())
        .unwrap_or(0);
    let config_store = ConfigStore::open("redirectionio");
    let req_sender = DirectRequestSender;
    let fastly_logger = FastlyLogger::new(
        config_store.get("log_endpoint"),
        config_store.get("log_level"),
        Context::new(req.clone_without_body()),
    );

    let config = match Configuration::new(
        config_store.get("backend_name"),
        config_store.get("token"),
        config_store.get("instance_name"),
        config_store.get("add_rule_ids_header"),
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

                    Ok(req_sender.send(req, backend_name.clone())?)
                }
            };
        }
    };

    let application = Application::new(&config, &fastly_logger, &req_sender);
    fastly_logger.log_info("Start worker".to_string(), None);

    let rio_request = match application.create_rio_request(&req) {
        Some(rio_request) => rio_request,
        None => return Ok(req_sender.send(req, config.backend_name.clone())?),
    };

    let mut rio_action = application.get_action(&rio_request);

    let action_match_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|time| time.as_millis())
        .unwrap_or(0);

    match application.proxy(req, &mut rio_action) {
        Ok((response, backend_status_code)) => {
            let proxy_response_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|time| time.as_millis());

            application.log(
                &response,
                backend_status_code,
                &rio_request,
                &mut rio_action,
                start_time,
                action_match_time,
                proxy_response_time,
            );
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
