extern crate iron;
extern crate router;

use iron::prelude::*;
use iron::status;
use router::Router;

use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::{thread, time};

#[derive(Clone)]
enum OutgoingMessage {
    Hello(String),
}

#[derive(Clone)]
pub struct InterfaceState {
    jig: String,
    scenarios: Vec<String>,
    tests: Vec<String>,
}

fn cfti_send(msg: OutgoingMessage) {
    let tx = io::stdout();
    match msg {
        OutgoingMessage::Hello(s) => {writeln!(tx.lock(), "HELLO {}", s);},
//        _ => println!("Unrecognized message"),
    }
}

fn show_index(request: &mut Request, state: &Arc<Mutex<InterfaceState>>) -> IronResult<Response> {
    let mut state = state.lock().unwrap();

    Ok(Response::with((status::Ok, format!("Hello, world!  Jig is: {}", state.jig).to_string())))
}

fn exit_server(request: &mut Request) -> IronResult<Response> {
    thread::spawn(|| {
        thread::sleep(time::Duration::from_millis(5));
        std::process::exit(0);
    });
    Ok(Response::with((status::Ok, "Server is shutting down".to_string())))
}

fn send_hello(request: &mut Request) -> IronResult<Response> {
    cfti_send(OutgoingMessage::Hello("hi there".to_string()));

    Ok(Response::with((status::Ok, "Sending HELLO".to_string())))
}

fn stdin_monitor(data_arc: Arc<Mutex<InterfaceState>>) {
    let mut rx = io::stdin();
    loop {
        let mut line = String::new();
        rx.read_line(&mut line).ok().expect("Unable to read line");

        let items: Vec<&str> = line.split_whitespace().collect();

        match items[0].to_lowercase().as_str() {
            "exit" => std::process::exit(0),
            "jig" => data_arc.lock().unwrap().jig = items[1].to_string().clone(),
            other => println!("Unrecognized command: {}", other),
        }
        thread::sleep(time::Duration::from_millis(100));
    }
}

fn main() {
    let mut router = Router::new();
    let jigs: Vec<String> = vec![];
    let mut state = Arc::new(Mutex::new(InterfaceState {
        jig: "".to_string(),
        scenarios: vec![],
        tests: vec![],
    }));

    let tmp = state.clone();
    router.get("/", move |request: &mut Request| show_index(request, &tmp), "index");
    router.get("/exit", exit_server, "exit");
    router.get("/hello", send_hello, "hello");
    //router.get("/jig", send_jig, "send_jigs");

    thread::spawn(move || stdin_monitor(state.clone()));
    Iron::new(router).http("localhost:3000").unwrap();
}
