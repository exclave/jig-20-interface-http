Jig-20 HTTP Interface
=======================

A web-based interface for the Jig-20 framework.

This version has been modified specifically for the NeTV2 test scenario.


Running
-------

Create webserver.interface in your jig-20 tests directory:

 [Interface]
 Name=Web Server
 Description=Runs a web server on port 3000
 Format=text
 ExecStart=cargo run
 WorkingDirectory=../jig-20-interface-http/


About
------

This interface uses the Common Factory Test Interface (CFTI) Interface-dialect.  It communicates with the server via its stdin and stdout.