use fastly::Request;
use serde::Serialize;
use serde_json::to_string as json_encode;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug, Serialize)]
pub struct FastlyLog {
    message: String,
    context: HashMap<&'static str, String>,
}

#[readonly::make]
pub struct FastlyLogger {
    has_logger: bool,
    log_endpoint: String,
    log_level: log::LevelFilter,
    context: Context,
}

impl FastlyLogger {
    pub(crate) fn new(
        log_endpoint: Option<String>,
        log_level: Option<String>,
        context: Context,
    ) -> FastlyLogger {
        let has_logger = match log_endpoint {
            Some(_) => true,
            None => false,
        };

        let log_endpoint = log_endpoint.unwrap_or("".to_string());
        let log_level = log_level.unwrap_or("warn".to_string());
        let log_level = match log::LevelFilter::from_str(log_level.as_str()) {
            Ok(level) => level,
            Err(_) => {
                println!(
                    "The log level \"{}\" is not valid, fallback to {}",
                    log_level,
                    log::LevelFilter::Warn
                );

                log::LevelFilter::Warn
            }
        };

        if has_logger {
            log_fastly::init_simple(log_endpoint.clone(), log_level);
        }

        return FastlyLogger {
            has_logger,
            log_endpoint,
            log_level,
            context,
        };
    }

    pub fn log_error(&self, message: String, context: Option<HashMap<&'static str, String>>) {
        self.log(message, context, log::Level::Error);
    }

    pub fn log_info(&self, message: String, context: Option<HashMap<&'static str, String>>) {
        self.log(message, context, log::Level::Info);
    }

    fn log(
        &self,
        message: String,
        context: Option<HashMap<&'static str, String>>,
        level: log::Level,
    ) {
        let mut context = match context {
            Some(context) => context,
            None => HashMap::new(),
        };

        context.insert("url", self.context.request.get_url_str().to_string());
        context.insert("method", self.context.request.get_method_str().to_string());
        context.insert("date", chrono::offset::Utc::now().to_string());
        context.insert("level", level.to_string());

        let log = FastlyLog { message, context };

        match json_encode(&log) {
            Ok(json) => {
                if level == log::Level::Error {
                    println!("{}", json);
                }

                if self.has_logger {
                    log::log!(level, "{}", json)
                }
            }
            Err(_) => return,
        };
    }
}

#[readonly::make]
pub struct Context {
    pub request: Request,
}

impl Context {
    pub(crate) fn new(request: Request) -> Context {
        return Context { request };
    }
}
