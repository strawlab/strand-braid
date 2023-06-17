export function launch_queue_set_consumer(func) {
    if ("launchQueue" in window) {
        window.launchQueue.setConsumer(func);
    }
}
