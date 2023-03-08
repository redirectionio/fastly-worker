use super::configuration::Configuration;
use super::logging::FastlyLogger;
use super::request_sender::RequestSender;

use fastly::http::header;
use fastly::http::Method;
use fastly::http::Version;
use fastly::{Error, Request, Response};
use redirectionio::action::Action;
use redirectionio::api::Log;
use redirectionio::http::{Header, Request as RedirectionioRequest};
use serde_json::from_str as json_decode;
use serde_json::to_string as json_encode;
use std::collections::HashMap;
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
    request_manager: &'a dyn RequestSender,
}

impl<'a> Application<'a> {
    pub(crate) fn new(
        configuration: &Configuration,
        fastly_logger: &'a FastlyLogger,
        request_sender: &'a dyn RequestSender,
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
            request_manager: request_sender,
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

        for (name, value) in req.get_headers() {
            let header_name = name.to_string();

            if header_name.starts_with(':') {
                continue;
            }

            if let Ok(s) = value.to_str() {
                rio_request.add_header(header_name, s.to_string(), true);
            } else {
                continue; // Invalid UTF-8
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

    pub fn proxy(&self, req: Request, action: &mut Action) -> Result<(Response, u16), Error> {
        let status_code_before_response = action.get_status_code(0, None);

        let request_method = req.get_method().clone();

        let mut response = if status_code_before_response == 0 {
            self.request_manager.send(req, self.backend_name.clone())?
        } else {
            let mut r = Response::new();
            r.set_status(status_code_before_response);
            r.append_header(header::CONTENT_TYPE, "text/html; charset=UTF-8");
            r.set_body(format!(
                "
<html>
<head><title>{}</title></head>
<body bgcolor=\"white\">
<center><h1>{}</h1></center>
</body>
</html>
<!-- a padding to disable MSIE and Chrome friendly error page -->
<!-- a padding to disable MSIE and Chrome friendly error page -->
<!-- a padding to disable MSIE and Chrome friendly error page -->
<!-- a padding to disable MSIE and Chrome friendly error page -->
<!-- a padding to disable MSIE and Chrome friendly error page -->
<!-- a padding to disable MSIE and Chrome friendly error page -->
<!-- a padding to disable MSIE and Chrome friendly error page -->
",
                &status_code_before_response, &status_code_before_response
            ));
            r
        };

        let backend_status_code = response.get_status().as_u16();
        let status_code_after_response = action.get_status_code(backend_status_code, None);

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

        let headers =
            action.filter_headers(headers, backend_status_code, self.add_rule_ids_header, None);

        for header in &headers {
            response.set_header(header.name.clone(), header.value.clone());
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
            match action.create_filter_body(backend_status_code, &headers) {
                Some(mut body_filter) => {
                    let mut new_response = response.clone_without_body();
                    let body = response.into_body().into_bytes();
                    let mut new_body = Vec::new();

                    new_body.extend(body_filter.filter(body, None));
                    new_body.extend(body_filter.end(None));
                    new_response.set_body(new_body);

                    response = new_response;
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
        if !action.should_log_request(true, backend_status_code, None) {
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
            None,
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
