use log::info;
use proxy_wasm::traits::*;
use proxy_wasm::types::*;
use serde::{Deserialize, Serialize};

proxy_wasm::main! {{
  proxy_wasm::set_log_level(LogLevel::Trace);
  proxy_wasm::set_root_context(|_| -> Box<dyn RootContext> {
    Box::new(
      FilterRoot{
        config: Config::default(),
        recv_queue: None,
    })
  })
}}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct Config {
    channel_name: String,
}

struct FilterRoot {
    config: Config,
    recv_queue: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct RequestEvent {
    request_key: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct RequestCount {
    request_count: u64,
}

impl FilterRoot {
    fn bump_ct(&self, evt: RequestEvent) {
        let evt_name = &evt.request_key;

        let (stored, cas_token) = self.get_shared_data(evt_name);

        let mut new_val = RequestCount { request_count: 1 };
        if let Some(val) = stored {
            if !val.is_empty() {
                new_val = serde_json::from_slice(&val).expect("invalid JSON");
                new_val.request_count += 1;
            }
        }
        let serialized = serde_json::to_string(&new_val).expect("couldn't serialize request count");
        self.set_shared_data(evt_name, Some(serialized.as_bytes()), cas_token).unwrap_or_else(
            |_| panic!("error while saving new request count: {}", evt_name)
        );
        info!("saved request count: {}", evt_name);
    }
}

impl Context for FilterRoot {}

impl RootContext for FilterRoot {
    fn on_configure(&mut self, _plugin_configuration_size: usize) -> bool {
        if let Some(bs) = self.get_plugin_configuration() {
            self.config = serde_json::from_slice(&bs).expect("invalid JSON");
            info!("Loaded config {:?}", self.config);
            self.recv_queue = Some(self.register_shared_queue(&self.config.channel_name));
            true
        } else {
            false
        }
    }

    fn on_queue_ready(&mut self, queue_id: u32) {
        if let Some(recv_queue_id) = self.recv_queue {
            if recv_queue_id == queue_id {
                self.bump_ct(
                    serde_json::from_slice(
                        &self.dequeue_shared_queue(recv_queue_id)
                            .expect("error while reading from queue")
                            .expect("empty message from queue")
                    ).expect("invalid JSON received on queue")
                );
            }
        }
    }
}
