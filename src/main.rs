#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate iron;
extern crate router;

use iron::prelude::*;
use iron::status;
use iron::mime::Mime;
use router::Router;
const SERVER_SIGNATURE: &'static str = "CFTI HTTP 1.0";

use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::{thread, time};
use std::collections::HashMap;

#[derive(Clone, Debug)]
enum OutgoingMessage {
    Hello(String),
    Jig,
    Scenarios,
    Scenario(String),
    Tests,
    Start,
    Abort,
    Log(String),
}

#[derive(Clone, Debug, Serialize)]
pub struct InterfaceState {

    /// The identifier of the server (returned on the HELLO line).
    server: String,

    /// Current jig identifier (returned by JIG)
    jig: String,

    /// Current jig display name (returned by DESCRIBE JIG NAME)
    jig_name: String,

    /// Current jig description (returned by DESCRIBE JIG DESCRIPTION)
    jig_description: String,

    /// List of currently-available scenarios (returned by "scenario")
    scenarios: Vec<String>,

    /// Map of scenario names, returned by DESCRIBE SCENARIO NAME [x] [y]
    scenario_names: HashMap<String, String>,

    /// Map of scenario descriptions, returned by DESCRIBE SCENARIO DESCRIPTION [x] [y]
    scenario_descriptions: HashMap<String, String>,

    /// List of tests in the current scenario, returned by TESTS [x]
    tests: Vec<String>,

    /// Map of test names, returned by various DESCRIBE TEST NAME [x] [y]
    test_names: HashMap<String, String>,

    /// Map of test descriptoins, returned by various DESCRIBE TEST DESCRIPTION [x] [y]
    test_descriptions: HashMap<String, String>,

    /// List of log entries, returned by LOG
    log: Vec<String>,
}

fn cfti_send(msg: OutgoingMessage) {
    let tx = io::stdout();
    let result = match msg {
        OutgoingMessage::Hello(s) => writeln!(tx.lock(), "HELLO {}", s),
        OutgoingMessage::Jig => writeln!(tx.lock(), "JIG"),
        OutgoingMessage::Scenarios => writeln!(tx.lock(), "SCENARIOS"),
        OutgoingMessage::Scenario(s) => writeln!(tx.lock(), "SCENARIO {}", s),
        OutgoingMessage::Tests => writeln!(tx.lock(), "TESTS"),
        OutgoingMessage::Start => writeln!(tx.lock(), "START"),
        OutgoingMessage::Abort => writeln!(tx.lock(), "ABORT"),
        OutgoingMessage::Log(s) => writeln!(tx.lock(), "LOG {}", s),
    };
    if result.is_err() {
        println!("Unable to write outgoing message: {}", result.unwrap_err());
    }
}

use std::fs::File;
use std::io::Read;

fn show_index(_: &mut Request, state: &Arc<Mutex<InterfaceState>>) -> IronResult<Response> {
    let ref state = *state.lock().unwrap();
    let mut index_file = File::open("index.html").unwrap();
    let mut index = String::new();
    index_file.read_to_string(&mut index).unwrap();

    let content_type = "text/html".parse::<Mime>().unwrap();
    Ok(Response::with((content_type, status::Ok, index)))
}

fn show_status_json(_: &mut Request, state: &Arc<Mutex<InterfaceState>>) -> IronResult<Response> {
    let ref state = *state.lock().unwrap();

    let content_type = "application/json".parse::<Mime>().unwrap();
    Ok(Response::with((content_type, status::Ok, serde_json::to_string(state).unwrap())))
}

fn exit_server(_: &mut Request) -> IronResult<Response> {
    thread::spawn(|| {
        thread::sleep(time::Duration::from_millis(5));
        std::process::exit(0);
    });
    Ok(Response::with((status::Ok, "Server is shutting down".to_string())))
}

fn send_hello(_: &mut Request) -> IronResult<Response> {
    cfti_send(OutgoingMessage::Hello(SERVER_SIGNATURE.to_string()));

    Ok(Response::with((status::Ok, "Sending HELLO".to_string())))
}

fn send_scenarios(_: &mut Request) -> IronResult<Response> {
    cfti_send(OutgoingMessage::Scenarios);

    Ok(Response::with((status::Ok, "Sending SCENARIOS".to_string())))
}

fn stdin_describe(data_arc: &Arc<Mutex<InterfaceState>>, items: Vec<String>) {
    let mut rest = items.clone();
    let class = items[0].to_lowercase();
    let field = items[1].to_lowercase();
    let name = items[2].to_lowercase();
    let name_uc = items[2].clone();

    // Remove the first three items: Class, Type, and Name.
    rest.remove(0);
    rest.remove(0);
    rest.remove(0);
    let value = rest.join(" ");
    match class.as_str() {
        "test" => match field.as_str() {
            "name" => {data_arc.lock().unwrap().test_names.insert(name, value).unwrap();},
            "description" => {data_arc.lock().unwrap().test_descriptions.insert(name, value).unwrap();},
            f => println!("Unrecognized field: {}", f),
        },
        "scenario" => match field.as_str() {
            "name" => {data_arc.lock().unwrap().scenario_names.insert(name, value).unwrap();},
            "description" => {data_arc.lock().unwrap().scenario_descriptions.insert(name, value).unwrap();},
            f => println!("Unrecognized field: {}", f),
        },
        "jig" => match field.as_str() {
            "name" => {data_arc.lock().unwrap().jig_name = format!("{} {}", name_uc, value);},
            "description" => {data_arc.lock().unwrap().jig_description = format!("{} {}", name_uc, value);},
            f => println!("Unrecognized field: {}", f),
        },
        c => println!("Unrecognized class: {}", c),
    };
}

fn stdin_monitor(data_arc: Arc<Mutex<InterfaceState>>) {
    let rx = io::stdin();
    loop {
        let mut line = String::new();
        rx.read_line(&mut line).ok().expect("Unable to read line");

        let mut items: Vec<String> = line.split_whitespace().map(|x| x.to_lowercase().to_string()).collect();
        let verb = items[0].to_lowercase();
        items.remove(0);

        match verb.as_str() {
            "hello" => data_arc.lock().unwrap().server = items.join(" "),
            "jig" => data_arc.lock().unwrap().jig = items[0].clone(),
            "scenarios" => data_arc.lock().unwrap().scenarios = items.clone(),
            "tests" => data_arc.lock().unwrap().tests = items.clone(),
            "describe" => stdin_describe(&data_arc, items),
            "log" => data_arc.lock().unwrap().log.push(items.join(" ")),
            "exit" => std::process::exit(0),
            other => println!("Unrecognized command: {}", other),
        }
        thread::sleep(time::Duration::from_millis(100));
    }
}

fn main() {
    let mut router = Router::new();
    let state = Arc::new(Mutex::new(InterfaceState {
        server: "".to_string(),
        jig: "".to_string(),
        jig_name: "".to_string(),
        jig_description: "".to_string(),
        scenarios: vec![],
        scenario_names: HashMap::new(),
        scenario_descriptions: HashMap::new(),
        tests: vec![],
        test_names: HashMap::new(),
        test_descriptions: HashMap::new(),
        log: vec![],
    }));

    let tmp = state.clone();
    router.get("/", move |request: &mut Request| show_index(request, &tmp), "index");

    let tmp = state.clone();
    router.get("/current.json", move |request: &mut Request| show_status_json(request, &tmp), "status");

    router.get("/exit", exit_server, "exit");
    router.get("/hello", send_hello, "hello");
    router.get("/scenarios", send_scenarios, "scenarios");
    //router.get("/jig", send_jig, "send_jigs");

    thread::spawn(move || stdin_monitor(state.clone()));
    Iron::new(router).http("localhost:3000").unwrap();
}
