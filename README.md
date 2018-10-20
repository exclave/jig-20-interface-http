# Jig-20 HTTP Interface
This version has been modified specifically for the NeTV2 test scenario.

A web-based interface for the Jig-20 framework.

## Usage
Build the program using cargo by running `cargo build --release`.

Run the program by starting target/release/jig-20-interface-http.  You can specify a port with `--port`, and it defaults to port 3000.

Create a website by adding files to `html/`.  These will be served up by the webserver.

You interact with the server by performing GET requests:

* `/current.json` - Returns a JSON object with the current tester state
* `/log.json` - Returns a JSON array with all log events.  You can obtain a subset of logs by specifying "&start=" and "&end=".  For example, to get the 2nd and 3rd logs ever generated, GET `/log.json?start=2&end=3`
* `/log/current.json` - Show logs for the current run (i.e. everything since START was pressed).  Also supports "&start=" and "&end="
* `/log/previous.json` - Show logs for the previous run.  Also supports "&start=" and "&end="
* `/stdin.txt` - Debug output of all text received on STDIN (if "-l" is specified).

Additionally, you can make requests to exclave by performing GET requests to the following addresses:

* `/truncate` - Truncate `log.json` and free associated memory.
* `/start` - Issue a "Start" command to exclave.  Exclave will ignore `start` if a scenario is already running.
* `/abort` - Abort the current scenario, if one is running.
* `/tests` - Request a new list of tests from exclave -- the result will appear in `/current.json`
* `/scenarios` - Request a new list of scenarios from exclave -- the result will appear in `/current.json`
* `/scenario` - Request the current scenario from exclave -- the result will appear in `/current.json`
* `/jig` - Request the current jig from exclave -- the result will appear in `/current.json`
* `/hello` - Send the "HELLO" message to exclave, to identify this server
* `/exit` - Shut down exclave and quit this web server

## Running

For testing purposes, you can simply run the program directly.  However, this is less useful without a server to generate CFTI messages.

To use with exclave, create `webserver.interface` in your exclave tests directory:

````ini
[Interface]
Name=Web Server
Description=Runs a web server on port 3000
Format=text
ExecStart=jig-20-interface-http --port 3000
WorkingDirectory=path-to-directory-containing-html-directory/
````

## About

This interface uses the Common Factory Test Interface (CFTI) Interface-dialect.  It communicates with the server via its stdin and stdout.