#[macro_use]
extern crate quick_error;

use fastly::http::header;
use fastly::http::Version;
use fastly::{Dictionary, Error, Request, Response};
use libflate::gzip::Decoder;
use redirectionio::action::Action;
use redirectionio::api::Log;
use redirectionio::http::{Header, Request as RedirectionioRequest};
use serde_json::from_str as json_decode;
use serde_json::to_string as json_encode;
use std::io::Read;
use std::str::FromStr;

// Internal stuff
const AGENT_VERSION: &str = "dev";
const API_ENDPOINT: &str = "https://agent.redirection.io";

struct Application {
    backend_name: String,
    token: String,
    instance_name: String,
    add_rule_ids_header: bool,
    agent_version: &'static str,
    api_endpoint: &'static str,
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

#[fastly::main]
fn main(req: Request) -> Result<Response, Error> {
    let application = match Application::new() {
        Ok(app) => app,
        Err(error) => match error {
            ConfigurationError::MissingBackendName => {
                let err_txt = format!("Fastly worker error: {}.\n", error);
                println!("{}", err_txt);

                let mut response = Response::new();
                response.set_body(err_txt);
                response.set_status(500);

                return Ok(response);
            }
            ConfigurationError::MissingToken(ref backend_name)
            | ConfigurationError::MissingInstanceName(ref backend_name)
            | ConfigurationError::MissingAddRuleIdsHeader(ref backend_name) => {
                let err_txt = format!("Fastly worker error: {}.\n", error);
                println!("{}", err_txt);

                return Ok(req.send(backend_name)?);
            }
        },
    };

    let rio_request = match application.create_rio_request(&req) {
        Some(rio_request) => rio_request,
        None => return Ok(req.send(application.backend_name)?),
    };

    let mut rio_action = match application.get_action(&rio_request) {
        Some(rio_action) => rio_action,
        None => return Ok(req.send(application.backend_name)?),
    };

    match application.proxy(req, &mut rio_action) {
        Ok(response) => {
            application.log(&response, &rio_request, &mut rio_action);
            Ok(response)
        }
        Err(error) => Err(error),
    }
}

impl Application {
    pub fn new() -> Result<Self, ConfigurationError> {
        let configuration = Dictionary::open("redirectionio");

        let backend_name = match configuration.get("backend_name") {
            Some(backend_name) => backend_name,
            None => return Err(ConfigurationError::MissingBackendName),
        };
        let token = match configuration.get("token") {
            Some(token) => token,
            None => return Err(ConfigurationError::MissingToken(backend_name)),
        };
        let instance_name = match configuration.get("instance_name") {
            Some(instance_name) => instance_name,
            None => return Err(ConfigurationError::MissingInstanceName(backend_name)),
        };
        let add_rule_ids_header = match configuration.get("add_rule_ids_header") {
            Some(add_rule_ids_header) => add_rule_ids_header,
            None => return Err(ConfigurationError::MissingAddRuleIdsHeader(backend_name)),
        };
        let add_rule_ids_header = add_rule_ids_header == "true";

        let application = Application {
            backend_name,
            token,
            instance_name,
            add_rule_ids_header,
            agent_version: AGENT_VERSION,
            api_endpoint: API_ENDPOINT,
        };

        Ok(application)
    }

    fn create_rio_request(&self, req: &Request) -> Option<RedirectionioRequest> {
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

    fn get_action(&self, rio_request: &RedirectionioRequest) -> Option<Action> {
        // FIXME: add some cache // => not now
        let json = match json_encode(&rio_request) {
            Ok(json) => json,
            Err(error) => {
                println!("Can not serialize redirection_io request: {}", error);
                return None;
            }
        };

        let response = Request::post(format!("{}/{}/action", self.api_endpoint, self.token))
            .with_header("User-Agent", format!("fastly-worker/{}", self.agent_version))
            .with_header("x-redirectionio-instance-name", self.instance_name.clone())
            .with_body(json)
            .with_version(Version::HTTP_11)
            .send("redirectionio");

        let mut response = match response {
            Ok(response) => response,
            Err(error) => {
                println!("Can not send redirection_io request: {}", error);
                return None;
            }
        };

        match json_decode(&response.take_body().into_string()) {
            Ok(action) => Some(action),
            Err(error) => {
                println!("Can not deserialize redirection_io API response: {}", error);
                None
            }
        }
    }

    fn proxy(&self, mut req: Request, action: &mut Action) -> Result<Response, Error> {
        let status_code_before_response = action.get_status_code(0);

        let accept_encoding = match req.get_header(header::ACCEPT_ENCODING) {
            Some(accept_encoding_value) if accept_encoding_value.to_str().unwrap().contains("gzip") => {
                req.set_header(header::ACCEPT_ENCODING, "gzip");
                true
            }
            _ => false,
        };

        let mut response = if status_code_before_response == 0 {
            req.send(self.backend_name.clone())?
        } else {
            let mut r = Response::new();
            r.set_status(status_code_before_response);
            r
        };

        let status_code_after_response = action.get_status_code(response.get_status().as_u16());

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

        let headers = action.filter_headers(headers, response.get_status().as_u16(), self.add_rule_ids_header);

        for header in headers {
            response.set_header(header.name, header.value);
        }

        match response.get_header(header::CONTENT_TYPE) {
            Some(content_type_value) if content_type_value.to_str().unwrap().contains("utf-8") => (),
            _ => return Ok(response),
        }

        let body = response.take_body();
        let body_orig = match response.get_header(header::CONTENT_ENCODING) {
            Some(_) => {
                let mut decoder = Decoder::new(body).unwrap();
                let mut decoded_data = Vec::new();
                decoder.read_to_end(&mut decoded_data).unwrap();

                String::from_utf8(decoded_data).unwrap()
            }
            None => body.into_string(),
        };

        let body = match action.create_filter_body(response.get_status().as_u16()) {
            Some(mut body_filter) => {
                let mut body = body_filter.filter(body_orig);
                let last = body_filter.end();

                if !last.is_empty() {
                    body = format!("{}{}", body, last);
                }

                body
            }
            None => body_orig,
        };

        if accept_encoding {
            response.remove_header(header::CONTENT_ENCODING);
            // We let fastly compress the response, save some $
            response.set_header("x-compress-hint", "on");
        }

        response.set_body(body);

        Ok(response)
    }

    fn log(&self, response: &Response, rio_request: &RedirectionioRequest, action: &mut Action) {
        let status_code = response.get_status().as_u16();

        if !action.should_log_request(true, status_code) {
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
            status_code,
            &response_headers,
            Some(action),
            format!("fastly-worker/{}", self.agent_version).as_str(),
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
            .with_header("User-Agent", format!("fastly-worker/{}", self.agent_version))
            .with_header("x-redirectionio-instance-name", self.instance_name.clone())
            .with_body(json)
            .with_version(Version::HTTP_11)
            .send("redirectionio");

        if result.is_err() {
            println!("Can not send \"log\" request to redirection.io: {}", result.err().unwrap());
        }
    }
}
