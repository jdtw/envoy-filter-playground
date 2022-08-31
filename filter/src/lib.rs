use log::{error, info};
use proxy_wasm::traits::*;
use proxy_wasm::types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

proxy_wasm::main! {{
    proxy_wasm::set_log_level(LogLevel::Trace);
    proxy_wasm::set_root_context(|_| -> Box<dyn RootContext> { Box::new(FilterRoot::default())})
}}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct Config {
    headers: HashMap<String, String>,
    channel_name: String,
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
        let body = body.as_deref();
        self.send_http_response(200, headers, body)
    }
}

#[derive(Debug)]
enum Do {
    Fail,
    Redirect(String),
    Body(String),
    Httpbin(String),
}

impl fmt::Display for Do {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Do::Fail => "Do.Fail",
                Do::Redirect(_) => "Do.Redirect",
                Do::Body(_) => "Do.Body",
                Do::Httpbin(_) => "Do.Httpbin",
            }
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct RequestCount {
    request_count: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct RequestEvent {
    request_key: String,
}

impl Filter {
    fn bump_request_ct(&self, action: &Option<Do>) -> u64 {
        let evt_name = get_key(action);
        let queue_id = self.resolve_shared_queue("my_vm_id", &self.config.channel_name).expect(
            "could not resolve queue"
        );

        let (stored, _) = self.get_shared_data(&evt_name);
        let mut new_val = RequestCount { request_count: 1 };
        if let Some(val) = stored {
            if !val.is_empty() {
                new_val = serde_json::from_slice(&val).expect("invalid JSON");
                new_val.request_count += 1;
            }
        };

        let evt = RequestEvent {
            request_key: evt_name,
        };

        let serialized = serde_json::to_string(&evt).expect(
            "couldn't serialize request event"
        );

        self.enqueue_shared_queue(queue_id, Some(serialized.as_bytes())).expect(
            "failed to send request event to service"
        );
        info!("sent request event to service");

        new_val.request_count
    }
}

fn get_key(req_action: &Option<Do>) -> String {
    let evt_suffix = match req_action{
        Some(act) => format!("{}", act),
        None => "GenericRequest".to_string()
    };
    format!("envoy.playground.request_ct.{}", evt_suffix)
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

        let req_ct = self.bump_request_ct(&action);
        info!("REQUEST CT VALUE for {}: {}", get_key(&action), req_ct);

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
