use log::{error, info};
use proxy_wasm::traits::*;
use proxy_wasm::types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

proxy_wasm::main! {{
    proxy_wasm::set_log_level(LogLevel::Trace);
    proxy_wasm::set_root_context(|_| -> Box<dyn RootContext> { Box::new(FilterRoot::default())})
}}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct Config {
    headers: HashMap<String, String>,
}

struct Filter {
    config: Config,
    body: Option<String>,
}

impl Context for Filter {
    fn on_http_call_response(
        &mut self,
        _token_id: u32,
        _num_headers: usize,
        body_size: usize,
        _num_trailers: usize,
    ) {
        for (k, v) in self.get_http_call_response_headers() {
            info!("- {}: {}", k, v);
        }
        let headers = self.get_http_call_response_headers();
        let headers = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let body = self.get_http_call_response_body(0, body_size);
        let body = body.as_ref().map(|bs| bs.as_slice());
        self.send_http_response(200, headers, body)
    }
}

enum Do {
    Fail,
    Redirect(String),
    Body(String),
    Httpbin(String),
}

impl HttpContext for Filter {
    fn on_http_request_headers(&mut self, _num_headers: usize, _end_of_stream: bool) -> Action {
        // Set some headers based on the config...
        for (k, v) in &self.config.headers {
            self.set_http_request_header(k.as_str(), Some(v.as_str()))
        }

        // Dump all request headers...
        let mut action = None;
        for (k, v) in self.get_http_request_headers() {
            info!("> {}: {}", k, v);
            match k.as_str() {
                "x-fail" => action = Some(Do::Fail),
                "x-redirect" => action = Some(Do::Redirect(v)),
                "x-body" => action = Some(Do::Body(v)),
                "x-httpbin" => action = Some(Do::Httpbin(v)),
                _ => (),
            }
        }

        // Take an action based on the request headers...
        match action {
            Some(Do::Fail) => {
                self.send_http_response(
                    403,
                    vec![("Powered-By", "proxy-wasm")],
                    Some(b"Access forbidden.\n"),
                );
                Action::Pause
            }
            Some(Do::Redirect(loc)) => {
                self.send_http_response(302, vec![("Location", &loc)], None);
                Action::Pause
            }
            Some(Do::Body(body)) => {
                self.body = Some(body);
                Action::Continue
            }
            Some(Do::Httpbin(path)) => {
                if let Err(e) = self.dispatch_http_call(
                    "httpbin",
                    vec![
                        (":method", "GET"),
                        (":path", &path),
                        (":authority", "httpbin.org"),
                    ],
                    None,
                    Vec::new(),
                    Duration::from_secs(5),
                ) {
                    error!("Failed to dispatch: {:?}", e);
                    self.send_http_response(500, Vec::new(), None)
                }
                Action::Pause
            }
            None => Action::Continue,
        }
    }

    fn on_http_response_headers(&mut self, _num_headers: usize, _end_of_stream: bool) -> Action {
        if self.body.is_some() {
            self.set_http_response_header("Content-Length", None);
            self.set_http_response_header("Content-Type", Some("text/plain"));
        }
        for (k, v) in self.get_http_response_headers() {
            info!("< {}: {}", k, v)
        }
        Action::Continue
    }

    fn on_http_response_body(&mut self, body_size: usize, end_of_stream: bool) -> Action {
        if let Some(body) = self.body.as_ref() {
            if !end_of_stream {
                return Action::Pause;
            }
            self.set_http_response_body(0, body_size, body.as_bytes());
        }
        Action::Continue
    }
}

#[derive(Default)]
struct FilterRoot {
    config: Config,
}

impl Context for FilterRoot {}

impl RootContext for FilterRoot {
    fn on_configure(&mut self, _plugin_configuration_size: usize) -> bool {
        match self.get_plugin_configuration() {
            Some(bs) => {
                self.config = serde_json::from_slice(&bs).expect("invalid JSON");
                info!("Loaded config {:?}", self.config);
                true
            }
            None => {
                error!("Missing plugin configuration!");
                false
            }
        }
    }

    fn create_http_context(&self, _context_id: u32) -> Option<Box<dyn HttpContext>> {
        Some(Box::new(Filter {
            config: self.config.clone(),
            body: None,
        }))
    }

    fn get_type(&self) -> Option<ContextType> {
        Some(ContextType::HttpContext)
    }
}

#[cfg(test)]
mod tests {}
