use stdweb::Value;
use yew::callback::Callback;
use yew::services::Task;

#[derive(Debug, PartialEq)]
pub enum ReadyState {
    Connecting,
    Open,
    Closed,
}

/// A handle to control current event source connection. Implements `Task` and could be canceled.
pub struct EventSourceTask(Option<Value>);

impl EventSourceTask {
    pub fn add_callback(&mut self, name: &str, callback: Callback<String>) -> Result<(), ()> {
        if let Some(ref handle) = self.0 {
            let data_callback = move |s: String| {
                callback.emit(s);
            };

            js! { @(no_return)
                var handle = @{handle};
                var data_callback = @{data_callback};

                handle.source.addEventListener(@{name}, function (event) {
                   data_callback(event.data);
                });
            }
            Ok(())
        } else {
            // TODO better error type.
            js! {
                @(no_return)
                console.error("error in EventSourceTask::add_callback");
            }
            Err(())
        }
    }
}

/// An event source service attached to a user context.
pub struct EventSourceService {}

impl EventSourceService {
    /// Creates a new service instance connected to `App` by provided `sender`.
    pub fn new() -> Self {
        Self {}
    }

    /// Connects to a server by an event source connection. Needs two functions to generate
    /// data and notification messages.
    pub fn connect(&mut self, url: &str, notification: Callback<ReadyState>) -> EventSourceTask {
        let notify_callback = move |code: u32| {
            let code = {
                match code {
                    0 => ReadyState::Connecting,
                    1 => ReadyState::Open,
                    2 => ReadyState::Closed,
                    x => panic!("unknown ready state code: {}", x),
                }
            };
            notification.emit(code);
        };
        let handle = js! {
            var source = new EventSource(@{url});
            var notify_callback = @{notify_callback};
            source.addEventListener("open", function (event) {
                notify_callback(source.readyState);
            });
            source.addEventListener("error", function (event) {
                notify_callback(source.readyState);
            });
            return {
                source,
            };
        };
        EventSourceTask(Some(handle))
    }
}

impl Task for EventSourceTask {
    fn is_active(&self) -> bool {
        self.0.is_some()
    }
    fn cancel(&mut self) {
        let handle = self.0.take().expect("tried to close event source twice");
        js! { @(no_return)
            var handle = @{handle};
            handle.source.close();
        }
    }
}

impl Drop for EventSourceTask {
    fn drop(&mut self) {
        if self.is_active() {
            self.cancel();
        }
    }
}
