#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate iron;
extern crate staticfile;
extern crate urlencoded;
extern crate mount;
extern crate clap;

use clap::{Arg, App};

use iron::prelude::*;
use iron::status;
use iron::mime::Mime;
use mount::Mount;
use staticfile::Static;

use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::{thread, time};
use std::collections::HashMap;

const SERVER_SIGNATURE: &'static str = "CFTI HTTP 1.0";

#[derive(Clone, Debug)]
enum OutgoingMessage {
    Hello(String),
    GetJig,
    Scenarios,
    Scenario(String),
    GetTests,
    StartTests(String),
    AbortTests,
    Log(String),
    Shutdown(String),
    Pong(String),
}

// <message-type>   <unit>    <unit-type>    <unix-time-secs>    <unix-time-nsecs>    <message>
#[derive(Clone, Debug, Serialize)]
struct LogMessage {
    message_class: String,
    unit_id: String,
    unit_type: String,
    timestamp: time::Duration,
    message: String,
}

#[derive(Clone, Debug, Serialize)]
enum ScenarioState {
    /// The scenario has not yet been run
    Pending,

    /// Some tests are being run
    Running,

    /// All scenario tests passed
    Pass,

    /// One or more of the tests failed
    Fail,
}

#[derive(Clone, Debug, Serialize)]
enum TestResult {
    /// The test has not yet been run.
    Pending,

    /// The test is currently being run.
    Running,

    /// The test passed.  "result" is the last string that it printed (if any).
    Pass(String /*result*/),

    /// The test failed.  "reason" is the last string it printed, or the reason it failed.
    Fail(String /*reason*/),

    /// The test was skipped, possibly due to an earlier dependency failure.
    Skipped(String /*reason*/),
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

    /// ID of the currently-selected scenario
    scenario: String,

    /// What state the current scenario is in
    scenario_state: ScenarioState,

    /// List of tests in each scenario, returned by TESTS [x]
    tests: HashMap<String, Vec<String>>,

    /// Map of test names, returned by various DESCRIBE TEST NAME [x] [y]
    test_names: HashMap<String, String>,

    /// Map of test descriptions, returned by various DESCRIBE TEST DESCRIPTION [x] [y]
    test_descriptions: HashMap<String, String>,

    /// Map of test results, usually will default to "Pending".
    test_results: HashMap<String, TestResult>,

    /// Incoming messages (for debugging)
    stdin_log: Vec<String>,

    /// Decide whether to log messages to stdin
    log_stdin: bool,

    /// Logs from the current run (i.e. since "START" was received)
    current_log: Vec<LogMessage>,

    /// Logs from the previous run (i.e. since "START" was received, until "STOP" was received)
    previous_log: Vec<LogMessage>,
}

fn cfti_escape(msg: String) -> String {
    msg.replace("\\", "\\\\").replace("\t", "\\t").replace("\n", "\\n").replace("\r", "\\r")
}

fn cfti_unescape(msg: String) -> String {
    let mut out = String::new();
    let mut was_bs = false;

    for c in msg.chars() {
        was_bs = match c {
            '\\' => if was_bs { out.push('\\'); false } else { true },
            't' => { out.push(if was_bs { '\t' } else { 't' }); false },
            'r' => { out.push(if was_bs { '\r' } else { 'r' }); false },
            'n' => { out.push(if was_bs { '\n' } else { 'n' }); false },
            _ => {out.push(c); false },
        };
    }
    out
}

fn cfti_send(msg: OutgoingMessage) {
    let tx = io::stdout();
    let result = match msg {
        OutgoingMessage::Hello(s) => writeln!(tx.lock(), "HELLO {}", cfti_escape(s)),
        OutgoingMessage::GetJig => writeln!(tx.lock(), "JIG"),
        OutgoingMessage::Scenarios => writeln!(tx.lock(), "SCENARIOS"),
        OutgoingMessage::Scenario(s) => writeln!(tx.lock(), "SCENARIO {}", cfti_escape(s)),
        OutgoingMessage::GetTests => writeln!(tx.lock(), "TESTS"),
        OutgoingMessage::StartTests(s) => writeln!(tx.lock(), "START {}", cfti_escape(s)),
        OutgoingMessage::AbortTests => writeln!(tx.lock(), "ABORT"),
        OutgoingMessage::Log(s) => writeln!(tx.lock(), "LOG {}", cfti_escape(s)),
        OutgoingMessage::Pong(s) => writeln!(tx.lock(), "PONG {}", cfti_escape(s)),
        OutgoingMessage::Shutdown(s) => writeln!(tx.lock(), "SHUTDOWN {}", cfti_escape(s)),
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

fn show_stdin(_: &mut Request, state: &Arc<Mutex<InterfaceState>>) -> IronResult<Response> {
    let ref state = *state.lock().unwrap();

    let content_type = "text/plain".parse::<Mime>().unwrap();
    Ok(Response::with((content_type, status::Ok, state.stdin_log.join("\n"))))
}

fn show_logs_json(request: &mut Request, logs: &Arc<Mutex<Vec<LogMessage>>>) -> IronResult<Response> {
    let content_type = "application/json".parse::<Mime>().unwrap();
    let query = match request.get_ref::<urlencoded::UrlEncodedQuery>() {
        Ok(hashmap) => hashmap.clone(),
        Err(_) => HashMap::new(),
    };

    let ref logs = *logs.lock().unwrap();

    let start = match query.get("start") {
        Some(s) => match s[0].parse() {
            Ok(o) => match o {
                o if o >= logs.len() => return Ok(Response::with((content_type, status::Ok, "[]".to_string()))),
                o => o,
            },
            Err(e) => return Ok(Response::with((status::BadRequest, format!("Unable to parse start value: {:?} / {}", s, e).to_string()))),
        },
        None => 0,
    };

    let end = match query.get("end") {
        Some(s) => match s[0].parse() {
            Ok(o) => match o {
                o if o >= logs.len() => logs.len() - 1,
                o => o,
            },
            Err(e) => return Ok(Response::with((status::BadRequest, format!("Unable to parse end value: {:?} / {}", s, e).to_string()))),
        },
        None => logs.len(),
    };

    Ok(Response::with((content_type, status::Ok, serde_json::to_string(&logs[start..end]).unwrap())))
}

fn show_current_logs_json(request: &mut Request, state: &Arc<Mutex<InterfaceState>>) -> IronResult<Response> {
    let content_type = "application/json".parse::<Mime>().unwrap();
    let query = match request.get_ref::<urlencoded::UrlEncodedQuery>() {
        Ok(hashmap) => hashmap.clone(),
        Err(_) => HashMap::new(),
    };

    let ref state = *state.lock().unwrap();

    let start = match query.get("start") {
        Some(s) => match s[0].parse() {
            Ok(o) => match o {
                o if o >= state.current_log.len() => return Ok(Response::with((content_type, status::Ok, "[]".to_string()))),
                o => o,
            },
            Err(e) => return Ok(Response::with((status::BadRequest, format!("Unable to parse start value: {:?} / {}", s, e).to_string()))),
        },
        None => 0,
    };

    let end = match query.get("end") {
        Some(s) => match s[0].parse() {
            Ok(o) => match o {
                o if o >= state.current_log.len() => state.current_log.len() - 1,
                o => o,
            },
            Err(e) => return Ok(Response::with((status::BadRequest, format!("Unable to parse end value: {:?} / {}", s, e).to_string()))),
        },
        None => state.current_log.len(),
    };

    Ok(Response::with((content_type, status::Ok, serde_json::to_string(&state.current_log[start..end]).unwrap())))
}

fn show_previous_logs_json(request: &mut Request, state: &Arc<Mutex<InterfaceState>>) -> IronResult<Response> {
    let content_type = "application/json".parse::<Mime>().unwrap();
    let query = match request.get_ref::<urlencoded::UrlEncodedQuery>() {
        Ok(hashmap) => hashmap.clone(),
        Err(_) => HashMap::new(),
    };

    let ref state = *state.lock().unwrap();

    let start = match query.get("start") {
        Some(s) => match s[0].parse() {
            Ok(o) => match o {
                o if o >= state.previous_log.len() => return Ok(Response::with((content_type, status::Ok, "[]".to_string()))),
                o => o,
            },
            Err(e) => return Ok(Response::with((status::BadRequest, format!("Unable to parse start value: {:?} / {}", s, e).to_string()))),
        },
        None => 0,
    };

    let end = match query.get("end") {
        Some(s) => match s[0].parse() {
            Ok(o) => match o {
                o if o >= state.previous_log.len() => state.previous_log.len() - 1,
                o => o,
            },
            Err(e) => return Ok(Response::with((status::BadRequest, format!("Unable to parse end value: {:?} / {}", s, e).to_string()))),
        },
        None => state.previous_log.len(),
    };

    Ok(Response::with((content_type, status::Ok, serde_json::to_string(&state.previous_log[start..end]).unwrap())))
}

fn truncate_logs(_request: &mut Request, state: &Arc<Mutex<Vec<LogMessage>>>) -> IronResult<Response> {
    let content_type = "application/json".parse::<Mime>().unwrap();
    let ref mut logs = *state.lock().unwrap();
    logs.clear();

    Ok(Response::with((content_type, status::Ok, "{status: \"ok\"}")))
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

fn start_tests(request: &mut Request, state: &Arc<Mutex<InterfaceState>>) -> IronResult<Response> {
    let scenario_id = match request.url.query() {
        None => state.lock().unwrap().scenario.clone(),
        Some(s) => s.to_string(),
    };

    cfti_send(OutgoingMessage::StartTests(scenario_id.clone()));

    Ok(Response::with((status::Ok, format!("Starting {} scenario", scenario_id))))
}

fn abort_tests(_: &mut Request) -> IronResult<Response> {
    cfti_send(OutgoingMessage::AbortTests);

    Ok(Response::with((status::Ok, "Aborting tests".to_string())))
}

fn stdin_describe(data: &mut InterfaceState, items: Vec<String>) {
    let mut rest = items.clone();
    let class = rest.remove(0).to_lowercase();
    let field = rest.remove(0).to_lowercase();
    let name = if rest.len() > 0 {
        rest.remove(0)
    } else {
        "No Name".to_string()
    };
    let name_lc = name.to_lowercase();
    let value = rest.join(" ");

    match class.as_str() {
        "test" => match field.as_str() {
            "name" => {data.test_names.insert(name_lc, value);},
            "description" => {data.test_descriptions.insert(name_lc, value);},
            f => eprintln!("Unrecognized field: {}", f),
        },
        "scenario" => match field.as_str() {
            "name" => {data.scenario_names.insert(name_lc, value);},
            "description" => {data.scenario_descriptions.insert(name_lc, value);},
            f => eprintln!("Unrecognized field: {}", f),
        },
        "jig" => match field.as_str() {
            "name" => {data.jig_name = value;},
            "description" => {data.jig_description = value;},
            f => eprintln!("Unrecognized field: {}", f),
        },
        c => eprintln!("Unrecognized class: {}", c),
    };
}

fn stdin_monitor(data_arc: Arc<Mutex<InterfaceState>>, logs: Arc<Mutex<Vec<LogMessage>>>) {
    let rx = io::stdin();
    loop {
        let mut line = String::new();
        rx.read_line(&mut line).ok().expect("Unable to read line");
        let ref mut data = *data_arc.lock().unwrap();
        if data.log_stdin {
            data.stdin_log.push(line.clone());
        }
        let line = cfti_unescape(line);

        let mut items: Vec<String> = line.split_whitespace().map(|x| x.to_string()).collect();
        //eprintln!("Got command: {:?}", items);
        let verb = items[0].to_lowercase();
        items.remove(0);

        match verb.as_str() {
            "hello" => data.server = items.join(" "),
            "jig" => data.jig = items.get(0).unwrap_or(&"No Jig".to_string()).clone(),
            "scenarios" => data.scenarios = items.clone(),
            "scenario" => {
                data.scenario = items.get(0).unwrap_or(&"No Scenario".to_string()).clone();
                data.scenario_state = ScenarioState::Pending;
            },
            "tests" => {
                let scenario_name = items.remove(0); // Remove the scenario name, which is the first result.
                data.tests.insert(scenario_name, items.clone());

                // We got a new set of tests, so reset all the test results to "Pending".
                data.test_results.clear();
                for item in items {
                    data.test_results.insert(item, TestResult::Pending);
                }
            },
            "describe" => stdin_describe(data, items),
            "ping" => cfti_send(OutgoingMessage::Pong(items.get(0).unwrap_or(&"".to_string()).clone())),
            "start" => {
                let scenario_name = items.remove(0);
                let ref mut data = *data;
                data.scenario_state = ScenarioState::Running;

                // We got a new set of tests, so reset all the test results to "Pending".
                data.test_results.clear();
                for test_name in &data.tests[&scenario_name] {
                    data.test_results.insert(test_name.clone(), TestResult::Pending);
                }

                // Move "current_log" to "previous_log", and truncate "current_log"
                data.previous_log.clear();
                for element in data.current_log.drain(..) {
                    data.previous_log.push(element);
                }
            },
            "finish" => {
                let result = match items.remove(1).parse() {
                    Ok(val) => val,
                    Err(e) => {eprintln!("Unable to parse result: {:?}", e); 500},
                };

                data.scenario_state = match result {
                    // Only results of 200 to 299 are considered "pass"
                    200 ... 299 => ScenarioState::Pass,
                    _ => ScenarioState::Fail,
                };
            }
            "running" => {
                let test_id = items.remove(0);
                data.test_results.insert(test_id, TestResult::Running);
            },
            "pass" => {
                let test_id = items.remove(0);
                let test_result = items.join(" ");
                data.test_results.insert(test_id, TestResult::Pass(test_result));
            },
            "fail" => {
                let test_id = items.remove(0);
                let test_result = items.join(" ");
                data.test_results.insert(test_id, TestResult::Fail(test_result));
            },
            "skip" => {
                let test_id = items.remove(0);
                let test_result = items.join(" ");
                data.test_results.insert(test_id, TestResult::Skipped(test_result));
            },
            "log" => {
                let message_class = items.remove(0);
                let unit_id = items.remove(0);
                let unit_type = items.remove(0);

                // Pull the timestamp out of the subsequent elements and parse them.
                let timestamp = time::Duration::new(items[0].parse().unwrap(),
                                                    items[1].parse().unwrap());
                items.remove(0); // Remove secs
                items.remove(0); // Remove nsecs

                let message = items.join(" ");
                let log_message = LogMessage {
                    message_class: message_class,
                    unit_id: unit_id,
                    unit_type: unit_type,
                    timestamp: timestamp,
                    message: message,
                };

                // Add the message to the global list of logs.
                logs.lock().unwrap().push(log_message.clone());

                // Also add the new message to the list of current log messages.
                data.current_log.push(log_message);
            },
            "exit" => std::process::exit(0),
            other => eprintln!("Unrecognized command: {}", other),
        }
    }
}

fn main() {
    let mut mnt = Mount::new();
    let staticfile = Static::new("html");

    let matches = App::new("Jig-20 HTTP Interface")
                        .version("1.0")
                        .author("Sean Cross <sean@xobs.io>")
                        .about("Presents CFTI over a web server")
                        .arg(Arg::with_name("ADDRESS")
                                .short("a")
                                .long("address")
                                .value_name("LISTEN_ADDRESS")
                                .help("Interface address to listen on")
                                .default_value("0.0.0.0")
                                .required(true)
                        )
                        .arg(Arg::with_name("PORT")
                                .short("p")
                                .long("port")
                                .value_name("PORT_NUMBER")
                                .help("Port to listen on")
                                .default_value("3000")
                                .required(true)
                        )
                        .arg(Arg::with_name("LOG_STDIN")
                                .short("l")
                                .long("log-stdin")
                                .help("Enable logging stdin to /stdin.txt")
                        )
                        .get_matches();

    let interface = matches.value_of("ADDRESS").unwrap();
    let port = matches.value_of("PORT").unwrap();

    let state = Arc::new(Mutex::new(InterfaceState {
        server: "".to_string(),
        jig: "".to_string(),
        jig_name: "".to_string(),
        jig_description: "".to_string(),
        scenarios: vec![],
        scenario_names: HashMap::new(),
        scenario_descriptions: HashMap::new(),
        scenario: "".to_string(),
        scenario_state: ScenarioState::Pending,
        tests: HashMap::new(),
        test_names: HashMap::new(),
        test_descriptions: HashMap::new(),
        test_results: HashMap::new(),
        stdin_log: vec![],
        log_stdin: matches.is_present("LOG_STDIN"),
        current_log: vec![],
        previous_log: vec![],
    }));

    let logs = Arc::new(Mutex::new(vec![]));

    cfti_send(OutgoingMessage::Log("HTTP interface starting up".to_string()));

    mnt.mount("/", staticfile);

    let tmp_state = state.clone();
    mnt.mount("/current.json", move |request: &mut Request| show_status_json(request, &tmp_state));

    let tmp_state = state.clone();
    mnt.mount("/stdin.txt", move |request: &mut Request| show_stdin(request, &tmp_state));

    let tmp_state = state.clone();
    mnt.mount("/log/current.json", move |request: &mut Request| show_current_logs_json(request, &tmp_state));

    let tmp_state = state.clone();
    mnt.mount("/log/previous.json", move |request: &mut Request| show_previous_logs_json(request, &tmp_state));

    let tmp_state = state.clone();
    mnt.mount("/start", move |request: &mut Request| start_tests(request, &tmp_state));

    let tmp_logs = logs.clone();
    mnt.mount("/log.json", move |request: &mut Request| show_logs_json(request, &tmp_logs));

    let tmp_logs = logs.clone();
    mnt.mount("/truncate", move |request: &mut Request| truncate_logs(request, &tmp_logs));

    mnt.mount("/exit", exit_server);
    mnt.mount("/hello", send_hello);
    mnt.mount("/scenarios", send_scenarios);
    mnt.mount("/scenario", select_scenario);
    mnt.mount("/jig", get_jig);
    mnt.mount("/tests", get_tests);
    mnt.mount("/abort", abort_tests);

    thread::spawn(move || stdin_monitor(state, logs));
    Iron::new(mnt).http(format!("{}:{}", interface, port).as_str()).unwrap();
}
