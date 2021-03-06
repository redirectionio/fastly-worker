use super::configuration::Configuration;
use super::error::InternalError;
use super::logging::FastlyLogger;

use fastly::http::header;
use fastly::http::Method;
use fastly::http::Version;
use fastly::{Error, Request, Response};
use libflate::gzip::Decoder;
use redirectionio::action::Action;
use redirectionio::api::Log;
use redirectionio::http::{Header, Request as RedirectionioRequest};
use serde_json::from_str as json_decode;
use serde_json::to_string as json_encode;
use std::collections::HashMap;
use std::io::Read;
use std::str::FromStr;

// Internal stuff
const AGENT_VERSION: &str = "dev";
const API_ENDPOINT: &str = "https://agent.redirection.io";

pub struct Application<'a> {
    backend_name: String,
    token: String,
    instance_name: String,
    add_rule_ids_header: bool,
    agent_version: &'static str,
    api_endpoint: &'static str,
    fastly_logger: &'a FastlyLogger,
}

impl<'a> Application<'a> {
    pub(crate) fn new(
        configuration: &Configuration,
        fastly_logger: &'a FastlyLogger,
    ) -> Application<'a> {
        let backend_name = configuration.backend_name.clone();
        let token = configuration.token.clone();
        let instance_name = configuration.instance_name.clone();
        let add_rule_ids_header = configuration.add_rule_ids_header;

        return Application {
            backend_name,
            token,
            instance_name,
            add_rule_ids_header,
            fastly_logger,
            agent_version: AGENT_VERSION,
            api_endpoint: API_ENDPOINT,
        };
    }

    pub fn create_rio_request(&self, req: &Request) -> Option<RedirectionioRequest> {
        let mut rio_request = match RedirectionioRequest::from_str(req.get_url().as_str()) {
            Ok(rio_request) => rio_request,
            Err(_) => return None,
        };

        rio_request.method = Some(req.get_method().to_string());
        rio_request.remote_addr = req.get_client_ip_addr();

        for name in req.get_original_header_names().unwrap() {
            if name.starts_with(':') {
                continue;
            }

            match req.get_header(&name) {
                Some(value) => {
                    if let Ok(s) = value.to_str() {
                        rio_request.add_header(name.to_string(), s.to_string(), true);
                    } else {
                        continue; // Invalid UTF-8
                    }
                }
                None => continue,
            }
        }

        Some(rio_request)
    }

    pub fn get_action(&self, rio_request: &RedirectionioRequest) -> Option<Action> {
        // FIXME: add some cache // => not now
        let json = match json_encode(&rio_request) {
            Ok(json) => json,
            Err(error) => {
                self.fastly_logger.log_error(
                    format!(
                        "Cannot get action from API. Cannot serialize redirection_io request: {}.",
                        error,
                    ),
                    None,
                );

                return None;
            }
        };

        let response = Request::post(format!("{}/{}/action", self.api_endpoint, self.token))
            .with_header(
                "User-Agent",
                format!("fastly-worker/{}", self.agent_version),
            )
            .with_header("x-redirectionio-instance-name", self.instance_name.clone())
            .with_body(json)
            .with_version(Version::HTTP_11)
            .send("redirectionio");

        let mut response = match response {
            Ok(response) => response,
            Err(error) => {
                self.fastly_logger.log_error(
                    format!(
                        "Cannot get action from API. Cannot send redirection_io request: {}.",
                        error,
                    ),
                    None,
                );

                return None;
            }
        };

        if response.get_status() != 200 {
            self.fastly_logger.log_error(
                format!(
                    "Cannot get action from API. Returned status {}.",
                    response.get_status(),
                ),
                Some(HashMap::from([
                    ("status", response.get_status().to_string()),
                    ("body", response.take_body_str()),
                ])),
            );

            return None;
        }

        match json_decode(&response.take_body().into_string()) {
            Ok(action) => Some(action),
            Err(error) => {
                self.fastly_logger.log_error(
                    format!("Cannot get action from API. Cannot deserialize redirection_io API response: {}.", error),
                    Some(HashMap::from([
                        ("status", response.get_status().to_string()),
                        ("body", response.take_body_str()),
                    ])),
                );
                None
            }
        }
    }

    pub fn proxy(&self, mut req: Request, action: &mut Action) -> Result<(Response, u16), Error> {
        let status_code_before_response = action.get_status_code(0);

        let accept_encoding = match req.get_header(header::ACCEPT_ENCODING) {
            Some(accept_encoding_value)
                if accept_encoding_value.to_str().unwrap().contains("gzip") =>
            {
                req.set_header(header::ACCEPT_ENCODING, "gzip");
                true
            }
            _ => false,
        };

        let request_method = req.get_method().clone();

        let mut response = if status_code_before_response == 0 {
            req.send(self.backend_name.clone())?
        } else {
            let mut r = Response::new();
            r.set_status(status_code_before_response);
            r
        };

        let backend_status_code = response.get_status().as_u16();
        let status_code_after_response = action.get_status_code(backend_status_code);

        if status_code_after_response != 0 {
            response.set_status(status_code_after_response);
        }

        let mut headers: Vec<Header> = vec![];

        for name in response.get_header_names() {
            match response.get_header(name) {
                Some(value) => {
                    if let Ok(s) = value.to_str() {
                        headers.push(Header {
                            name: name.to_string(),
                            value: s.to_string(),
                        });
                    } else {
                        continue; // Invalid UTF-8
                    }
                }
                None => continue,
            }
        }

        let headers = action.filter_headers(headers, backend_status_code, self.add_rule_ids_header);

        for header in headers {
            response.set_header(header.name, header.value);
        }

        match response.get_header(header::CONTENT_TYPE) {
            Some(content_type_value)
                if content_type_value
                    .to_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("utf-8") =>
            {
                ()
            }
            _ => return Ok((response, backend_status_code)),
        }

        if request_method != &Method::HEAD {
            match action.create_filter_body(backend_status_code) {
                Some(mut body_filter) => {
                    let body_orig = match decode_original_body(response.clone_with_body()) {
                        Ok(body_orig) => body_orig,
                        Err(e) => {
                            self.fastly_logger
                                .log_error(format!("Can not decode original body: {}.", e), None);
                            return Ok((response, backend_status_code));
                        }
                    };

                    let mut body = body_filter.filter(body_orig);
                    let last = body_filter.end();

                    if !last.is_empty() {
                        body = format!("{}{}", body, last);
                    }

                    response.set_body(body);

                    if accept_encoding {
                        response.remove_header(header::CONTENT_ENCODING);
                        // We let fastly compress the response, save some $
                        response.set_header("x-compress-hint", "on");
                    }
                }
                None => (),
            };
        }

        Ok((response, backend_status_code))
    }

    pub fn log(
        &self,
        response: &Response,
        backend_status_code: u16,
        rio_request: &RedirectionioRequest,
        action: &mut Action,
    ) {
        if !action.should_log_request(true, backend_status_code) {
            return;
        }

        let mut response_headers: Vec<Header> = vec![];
        for name in response.get_header_names() {
            match response.get_header(name) {
                Some(value) => {
                    if let Ok(s) = value.to_str() {
                        response_headers.push(Header {
                            name: name.to_string(),
                            value: s.to_string(),
                        });
                    } else {
                        continue; // Invalid UTF-8
                    }
                }
                None => continue,
            }
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let log = Log::from_proxy(
            rio_request,
            response.get_status().as_u16(),
            &response_headers,
            Some(action),
            format!("redirectionio-fastly:{}", self.agent_version).as_str(),
            timestamp,
            match rio_request.remote_addr {
                Some(ref addr) => addr.to_string(),
                None => String::from(""),
            }
            .as_str(),
        );

        let json = match json_encode(&log) {
            Err(_) => return,
            Ok(s) => s,
        };

        let result = Request::post(format!("{}/{}/log", self.api_endpoint, self.token))
            .with_header(
                "User-Agent",
                format!("fastly-worker/{}", self.agent_version),
            )
            .with_header("x-redirectionio-instance-name", self.instance_name.clone())
            .with_body(json)
            .with_version(Version::HTTP_11)
            .send("redirectionio");

        if result.is_err() {
            self.fastly_logger.log_error(
                format!(
                    "Can not send \"log\" request to redirection.io: {}.",
                    result.err().unwrap()
                ),
                None,
            );
        }
    }
}

fn decode_original_body(mut response: Response) -> Result<String, InternalError> {
    let body = response.take_body();

    match response.get_header(header::CONTENT_ENCODING) {
        None => {
            // Try to decode the UTF-8 content of the response: avoid using body.into_string which is panicking
            match String::from_utf8(body.into_bytes()) {
                Ok(body) => Ok(body),
                Err(e) => return Err(InternalError::from(e)),
            }
        }
        Some(encoding) => match encoding.to_str().unwrap() {
            "gzip" => {
                let mut decoder = Decoder::new(body)?;
                let mut decoded_data = Vec::new();
                decoder.read_to_end(&mut decoded_data)?;

                Ok(String::from_utf8(decoded_data)?)
            }
            _ => return Err(InternalError::EncodingNotSupported),
        },
    }
}
