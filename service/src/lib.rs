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
    fn bump_ct(&self, evt: &RequestEvent) -> u64 {
        let evt_name = &evt.request_key;

        let (stored, cas_token) = self.get_shared_data(&evt_name);

        let mut new_val = RequestCount { request_count: 1 };
        if let Some(val) = stored {
            if !val.is_empty() {
                new_val = serde_json::from_slice(&val).expect("invalid JSON");
                new_val.request_count += 1;
            }
        }
        let serialized = match serde_json::to_string(&new_val) {
            Ok(s) => s,
            Err(_) => panic!("couldn't serialize request count"),
        };
        match self.set_shared_data(&evt_name, Some(serialized.as_bytes()), cas_token) {
            Ok(_) => info!("saved request count: {}", evt_name),
            Err(_) => panic!("error while saving new request count: {}", evt_name),
        };
        return new_val.request_count;
    }
}

impl Context for FilterRoot {}

impl RootContext for FilterRoot {
    fn on_configure(&mut self, _plugin_configuration_size: usize) -> bool {
        if let Some(bs) = self.get_plugin_configuration() {
            self.config = serde_json::from_slice(&bs).expect("invalid JSON");
            info!("Loaded config {:?}", self.config);
        } else {
            return false;
        }

        self.recv_queue = Some(self.register_shared_queue(&self.config.channel_name));
        return true;
    }

    fn get_type(&self) -> Option<ContextType> {
        Some(ContextType::HttpContext)
    }

    fn on_queue_ready(&mut self, queue_id: u32) {
        if let Some(recv_queue_id) = self.recv_queue {
            if recv_queue_id == queue_id {
                let received: RequestEvent = match self.dequeue_shared_queue(recv_queue_id) {
                    Ok(maybe_buff) => match maybe_buff {
                        Some(buff) => {
                            serde_json::from_slice(&buff).expect("invalid JSON received on queue")
                        }
                        None => panic!("empty message from queue"),
                    },
                    Err(_) => panic!("error while reading from queue"),
                };
                self.bump_ct(&received);
            }
        }
    }
}
