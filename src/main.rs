#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate iron;
extern crate staticfile;
extern crate mount;

use iron::prelude::*;
use iron::status;
use iron::mime::Mime;
use mount::Mount;
use staticfile::Static;
const SERVER_SIGNATURE: &'static str = "CFTI HTTP 1.0";

use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::{thread, time};
use std::collections::HashMap;

macro_rules! println_stderr(
    ($($arg:tt)*) => { {
        let r = writeln!(&mut ::std::io::stderr(), $($arg)*);
        r.expect("failed printing to stderr");
    } }
);

#[derive(Clone, Debug)]
enum OutgoingMessage {
    Hello(String),
    GetJig,
    Scenarios,
    Scenario(String),
    GetTests,
    StartTests,
    AbortTests,
    Log(String),
    Shutdown(String),
}

// <message-type>   <unit>    <unit-type>    <unix-time-secs>    <unix-time-nsecs>    <message>
#[derive(Clone, Debug, Serialize)]
struct LogMessage {
    message_type: u32,
    unit_id: String,
    unit_type: String,
    timestamp: time::Duration,
    message: String,
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
    log: Vec<LogMessage>,
}

fn cfti_send(msg: OutgoingMessage) {
    let tx = io::stdout();
    let result = match msg {
        OutgoingMessage::Hello(s) => writeln!(tx.lock(), "HELLO {}", s),
        OutgoingMessage::GetJig => writeln!(tx.lock(), "JIG"),
        OutgoingMessage::Scenarios => writeln!(tx.lock(), "SCENARIOS"),
        OutgoingMessage::Scenario(s) => writeln!(tx.lock(), "SCENARIO {}", s),
        OutgoingMessage::GetTests => writeln!(tx.lock(), "TESTS"),
        OutgoingMessage::StartTests => writeln!(tx.lock(), "START"),
        OutgoingMessage::AbortTests => writeln!(tx.lock(), "ABORT"),
        OutgoingMessage::Log(s) => writeln!(tx.lock(), "LOG {}", s),
        OutgoingMessage::Shutdown(s) => writeln!(tx.lock(), "SHUTDOWN {}", s),
    };
    if result.is_err() {
        println!("Unable to write outgoing message: {}", result.unwrap_err());
    }
}

fn show_status_json(_: &mut Request, state: &Arc<Mutex<InterfaceState>>) -> IronResult<Response> {
    let ref state = *state.lock().unwrap();

    let content_type = "application/json".parse::<Mime>().unwrap();
    Ok(Response::with((content_type, status::Ok, serde_json::to_string(state).unwrap())))
}

fn exit_server(_: &mut Request) -> IronResult<Response> {
    cfti_send(OutgoingMessage::Shutdown("User clicked Quit".to_string()));

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

fn select_scenario(request: &mut Request) -> IronResult<Response> {

    println_stderr!("Request URL: {:?}", request.url.query());
    let scenario_id = match request.url.query() {
        None => return Ok(Response::with((status::BadRequest, "scenario request needs a scenario id.  Access /scenario?id".to_string()))),
        Some(s) => s.to_string(),
    };

    cfti_send(OutgoingMessage::Scenario(scenario_id.clone()));
    Ok(Response::with((status::Ok, format!("Selecting scenario {}", scenario_id).to_string())))
}

fn get_jig(_: &mut Request) -> IronResult<Response> {
    cfti_send(OutgoingMessage::GetJig);

    Ok(Response::with((status::Ok, "Requesting jig id".to_string())))
}

fn get_tests(_: &mut Request) -> IronResult<Response> {
    cfti_send(OutgoingMessage::GetTests);

    Ok(Response::with((status::Ok, "Requesting test list".to_string())))
}

fn start_tests(_: &mut Request) -> IronResult<Response> {
    cfti_send(OutgoingMessage::StartTests);

    Ok(Response::with((status::Ok, "Starting tests".to_string())))
}

fn abort_tests(_: &mut Request) -> IronResult<Response> {
    cfti_send(OutgoingMessage::AbortTests);

    Ok(Response::with((status::Ok, "Aborting tests".to_string())))
}

fn stdin_describe(data_arc: &Arc<Mutex<InterfaceState>>, items: Vec<String>) {
    let mut rest = items.clone();
    let class = rest.remove(0).to_lowercase();
    let field = rest.remove(0).to_lowercase();
    let jig_value = rest.join(" ");
    let name = rest.remove(0);
    let name_lc = name.to_lowercase();

    let value = rest.join(" ");


    match class.as_str() {
        "test" => match field.as_str() {
            "name" => {data_arc.lock().unwrap().test_names.insert(name_lc, value);},
            "description" => {data_arc.lock().unwrap().test_descriptions.insert(name_lc, value);},
            f => println_stderr!("Unrecognized field: {}", f),
        },
        "scenario" => match field.as_str() {
            "name" => {data_arc.lock().unwrap().scenario_names.insert(name_lc, value);},
            "description" => {data_arc.lock().unwrap().scenario_descriptions.insert(name_lc, value);},
            f => println_stderr!("Unrecognized field: {}", f),
        },
        "jig" => match field.as_str() {
            "name" => {data_arc.lock().unwrap().jig_name = jig_value;},
            "description" => {data_arc.lock().unwrap().jig_description = jig_value;},
            f => println_stderr!("Unrecognized field: {}", f),
        },
        c => println_stderr!("Unrecognized class: {}", c),
    };
}

fn stdin_monitor(data_arc: Arc<Mutex<InterfaceState>>) {
    let rx = io::stdin();
    loop {
        let mut line = String::new();
        rx.read_line(&mut line).ok().expect("Unable to read line");

        let mut items: Vec<String> = line.split_whitespace().map(|x| x.to_string()).collect();
        println_stderr!("Got command: {:?}", items);
        let verb = items[0].to_lowercase();
        items.remove(0);

        match verb.as_str() {
            "hello" => data_arc.lock().unwrap().server = items.join(" "),
            "jig" => data_arc.lock().unwrap().jig = items[0].clone(),
            "scenarios" => data_arc.lock().unwrap().scenarios = items.clone(),
            "tests" => data_arc.lock().unwrap().tests = items.clone(),
            "describe" => stdin_describe(&data_arc, items),
            "log" => {
                let message_type: u32 = items.remove(0).parse().unwrap();
                let unit_id = items.remove(0);
                let unit_type = items.remove(0);
                let timestamp = time::Duration::new(items[0].parse().unwrap(),
                                                    items[1].parse().unwrap());
                items.remove(0);
                items.remove(0);
                let message = items.join(" ");
                data_arc.lock().unwrap().log.push(LogMessage {
                    message_type: message_type,
                    unit_id: unit_id,
                    unit_type: unit_type,
                    timestamp: timestamp,
                    message: message,
                });
            },
            "exit" => std::process::exit(0),
            other => println_stderr!("Unrecognized command: {}", other),
        }
        thread::sleep(time::Duration::from_millis(100));
    }
}

fn main() {
    let mut mnt = Mount::new();
    let staticfile = Static::new("html");

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

    cfti_send(OutgoingMessage::Log("HTTP interface starting up".to_string()));

    mnt.mount("/", staticfile);

    let tmp = state.clone();
    mnt.mount("/current.json", move |request: &mut Request| show_status_json(request, &tmp));

    mnt.mount("/exit", exit_server);
    mnt.mount("/hello", send_hello);
    mnt.mount("/scenarios", send_scenarios);
    mnt.mount("/scenario", select_scenario);
    mnt.mount("/jig", get_jig);
    mnt.mount("/tests", get_tests);
    mnt.mount("/start", start_tests);
    mnt.mount("/abort", abort_tests);

    thread::spawn(move || stdin_monitor(state.clone()));
    Iron::new(mnt).http("localhost:3000").unwrap();
}
