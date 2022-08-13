use log::{error, info};
use proxy_wasm::traits::*;
use proxy_wasm::types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
}

impl Context for Filter {}

impl HttpContext for Filter {
    fn on_http_request_headers(&mut self, _num_headers: usize, _end_of_stream: bool) -> Action {
        // Set some headers based on the config...
        for (k, v) in &self.config.headers {
            self.set_http_request_header(k.as_str(), Some(v.as_str()))
        }
        // Dump all request headers...
        let mut fail = false;
        for (k, v) in self.get_http_request_headers() {
            info!("> {}: {}", k, v);
            if k == "x-fail" {
                fail = true;
            }
        }
        // Fail the request if the x-fail header is present...
        if fail {
            self.send_http_response(
                403,
                vec![("Powered-By", "proxy-wasm")],
                Some(b"Access forbidden.\n"),
            );
            return Action::Pause;
        }
        Action::Continue
    }

    fn on_http_response_headers(&mut self, _num_headers: usize, _end_of_stream: bool) -> Action {
        for (k, v) in self.get_http_response_headers() {
            info!("< {}: {}", k, v)
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
                self.config = serde_json::from_slice(&bs).unwrap();
                info!("Loaded config {:?}", self.config);
                true
            }
            None => {
                error!("Missing plugin configuration!");
                false
            }
        }
    }

    fn create_http_context(&self, _: u32) -> Option<Box<dyn HttpContext>> {
        Some(Box::new(Filter {
            config: self.config.clone(),
        }))
    }

    fn get_type(&self) -> Option<ContextType> {
        Some(ContextType::HttpContext)
    }
}

#[cfg(test)]
mod tests {}
