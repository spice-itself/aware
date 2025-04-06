# Aware Process Supervisor

Aware is a simple and lightweight process supervisor for Linux/Unix systems. It monitors your applications, automatically restarts them if they crash, and provides logging capabilities.

## Features

- **Process Monitoring**: Automatically restarts processes if they terminate
- **Logging**: Captures stdout and stderr output to log files
- **Simple CLI**: Easy-to-use command line interface
- **Graceful Termination**: Properly stops supervised processes

## Installation

Clone the repository and build the project:

```bash
git clone https://github.com/yourusername/aware.git
cd aware
cargo build --release
```

You can also add the binary to your PATH for easier access:

```bash
cp target/release/aware /usr/local/bin/
```

## Usage

### Starting a Process

To start and supervise a process:

```bash
aware supervise <program> [arguments...]
```

This will:
1. Start the specified program with any provided arguments
2. Create log files in the `aware_logs` directory
3. Automatically restart the program if it terminates

Example:

```bash
# Run a web server and restart it if it crashes
aware supervise /usr/local/bin/my_web_server --port 8080

# Run a script with arguments
aware supervise python3 my_script.py --config config.json
```

### Stopping a Process

To stop a supervised process:

```bash
# Stop a specific process
aware leave <program_name>

# Stop all supervised processes
aware leave
```

## Log Files

Logs are stored in the `aware_logs` directory. Each supervised process has its own log file named after the program:

```
aware_logs/
  ├── my_web_server.log
  ├── python3.log
  └── ...
```

Each log entry includes a timestamp and the output from the supervised process.

## Process Management

Aware keeps track of supervised processes using PID files in the `aware_pids` directory:

```
aware_pids/
  ├── my_web_server.pid
  ├── python3.pid
  ├── processes.list
  └── ...
```

## Example Workflow

1. Start a service with supervision:
   ```bash
   aware supervise /usr/bin/nginx -c /etc/nginx/nginx.conf
   ```

2. Check the logs:
   ```bash
   tail -f aware_logs/nginx.log
   ```

3. Stop the service when needed:
   ```bash
   aware leave nginx
   ```

## Notes

- If a process crashes, Aware will wait 2 seconds before attempting to restart it
- All command operations are logged both to console and to the log file
- The supervisor itself can be terminated with Ctrl+C or by sending a SIGINT signal
