use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

enum SupervisorCommand {
    Leave,
}

struct ProcessInfo {
    name: String,
    args: Vec<String>,
    log_path: String,
}

struct CommandChannel {
    command: Mutex<Option<SupervisorCommand>>,
}

impl CommandChannel {
    fn new() -> Self {
        CommandChannel {
            command: Mutex::new(None),
        }
    }

    fn put_command(&self, cmd: SupervisorCommand) {
        let mut command = self.command.lock().unwrap();
        *command = Some(cmd);
    }

    fn get_command(&self) -> Option<SupervisorCommand> {
        let mut command = self.command.lock().unwrap();
        command.take()
    }
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args[1].as_str() {
        "supervise" => {
            if args.len() < 3 {
                println!("Error: Specify a program to run");
                return Ok(());
            }

            // Create logs directory
            fs::create_dir_all("aware_logs")?;

            // Get program name from path
            let program_path = &args[2];
            let program_name = Path::new(program_path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();

            // Create log path
            let log_path = format!("aware_logs/{}.log", program_name);

            // Prepare process info
            let process_info = ProcessInfo {
                name: program_path.clone(),
                args: args[3..].to_vec(),
                log_path,
            };

            // Write PID file
            write_pid_file(&program_name)?;

            // Run supervisor
            run_supervisor(process_info)?;
        }
        "leave" => {
            // Check if specific program name is provided
            if args.len() >= 3 {
                // Send command to terminate specific program
                send_leave_command(Some(&args[2]))?;
            } else {
                // Send command to terminate all programs
                send_leave_command(None)?;
            }
        }
        _ => {
            println!("Unknown command: {}", args[1]);
            println!("Supported commands: supervise, leave");
        }
    }

    Ok(())
}

fn print_usage() {
    println!(
        "Usage:\n  aware supervise <program> [arguments...]\n  aware leave [program_name]"
    );
}

fn write_pid_file(program_name: &str) -> io::Result<()> {
    // Create PIDs directory if it doesn't exist
    fs::create_dir_all("aware_pids")?;

    // Create PID file with program name
    let pid_file_path = format!("aware_pids/{}.pid", program_name);
    let mut pid_file = File::create(&pid_file_path)?;

    // Get current process PID
    let pid = std::process::id();
    writeln!(pid_file, "{}", pid)?;

    // Add entry to the common process list
    let all_pids_path = "aware_pids/processes.list";
    let mut all_pids_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(all_pids_path)?;

    writeln!(all_pids_file, "{}:{}", program_name, pid)?;

    Ok(())
}

fn send_leave_command(program_name: Option<&str>) -> io::Result<()> {
    if let Some(name) = program_name {
        println!("Sending leave command to process {}", name);

        // Form path to the PID file for the specific program
        let pid_file_path = format!("aware_pids/{}.pid", name);

        // Check if PID file exists
        if !Path::new(&pid_file_path).exists() {
            println!(
                "PID file for {} not found, process may not be running",
                name
            );
            return Ok(());
        }

        // Read PID from file
        let mut pid_file = File::open(&pid_file_path)?;
        let mut pid_str = String::new();
        pid_file.read_to_string(&mut pid_str)?;

        let pid = pid_str.trim().parse::<u32>().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Error parsing PID: {}", e),
            )
        })?;

        println!("Sending signal to process {} with PID: {}", name, pid);

        // In a real implementation, here would be code to send a signal
        // In this simplified version, we just delete the PID file
        fs::remove_file(pid_file_path)?;
        println!("Command sent to process {}", name);
    } else {
        println!("Sending leave command to all running aware processes");

        // Check if the directory with PID files exists
        let pid_dir_path = PathBuf::from("aware_pids");
        if !pid_dir_path.exists() {
            println!("PID files directory not found, there may be no active supervisors");
            return Ok(());
        }

        // Iterate through all PID files
        for entry in fs::read_dir(pid_dir_path.clone())? {
            let entry = entry?;
            let path = entry.path();

            // Skip the process list file
            if path.file_name().unwrap() == "processes.list" {
                continue;
            }

            // Check if it's a PID file
            if let Some(extension) = path.extension() {
                if extension == "pid" {
                    // Read PID from file
                    let mut pid_file = File::open(&path)?;
                    let mut pid_str = String::new();
                    pid_file.read_to_string(&mut pid_str)?;

                    let pid = match pid_str.trim().parse::<u32>() {
                        Ok(pid) => pid,
                        Err(e) => {
                            println!(
                                "Error reading PID from {:?}: {}",
                                path.file_name().unwrap(),
                                e
                            );
                            continue;
                        }
                    };

                    // Get program name (without .pid extension)
                    let file_name = path.file_name().unwrap().to_string_lossy().to_string();
                    let prog_name = file_name.trim_end_matches(".pid");
                    println!("Sending signal to process {} with PID: {}", prog_name, pid);

                    // Delete PID file
                    fs::remove_file(&path)?;
                }
            }
        }

        // Clear the process list file
        let list_path = pid_dir_path.join("processes.list");
        File::create(list_path)?;

        println!("Command sent to all processes");
    }

    Ok(())
}

fn run_supervisor(info: ProcessInfo) -> io::Result<()> {
    println!("Starting supervisor for program: {}", info.name);
    println!("Logs will be saved to: {}", info.log_path);

    // Open file for logs
    let log_file = Arc::new(Mutex::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&info.log_path)?,
    ));

    write_log(&log_file, "Starting supervisor")?;

    // Create channel for commands
    let command_channel = Arc::new(CommandChannel::new());
    let running = Arc::new(AtomicBool::new(true));

    // Start thread for command processing
    let command_thread_running = Arc::clone(&running);
    let command_thread_channel = Arc::clone(&command_channel);
    let command_thread = thread::spawn(move || {
        command_listener(command_thread_running, command_thread_channel);
    });

    let running_clone = Arc::clone(&running);
    while running.load(Ordering::Acquire) {
        // Start process
        let mut child = match start_process(&info, &log_file) {
            Ok(child) => child,
            Err(e) => {
                let err_msg = format!("Error starting process: {}", e);
                let _ = write_log(&log_file, &err_msg);

                // Pause before retry
                thread::sleep(Duration::from_secs(5));
                continue;
            }
        };

        // Wait for process to finish or command to stop
        loop {
            // Check process status
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process finished
                    let exit_msg = if status.success() {
                        "Process finished successfully with code 0".to_string()
                    } else {
                        format!(
                            "Process finished with error: code {}",
                            status.code().unwrap_or(-1)
                        )
                    };
                    let _ = write_log(&log_file, &exit_msg);
                    break;
                }
                Ok(None) => {
                    // Process still running, check commands
                    if !running.load(Ordering::Acquire) {
                        let _ = write_log(&log_file, "Received command to terminate");
                        let _ = child.kill();
                        let _ = write_log(&log_file, "Process stopped");
                        return Ok(());
                    }

                    // Small pause to avoid CPU overload
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    let err_msg = format!("Error checking process status: {}", e);
                    let _ = write_log(&log_file, &err_msg);
                    break;
                }
            }
        }

        // Pause before restarting the process
        if running.load(Ordering::Acquire) {
            let _ = write_log(&log_file, "Restarting process in 2 seconds...");
            thread::sleep(Duration::from_secs(2));
        }
    }

    let _ = write_log(&log_file, "Supervisor shutting down");

    // Wait for command thread to finish
    running_clone.store(false, Ordering::Release);
    let _ = command_thread.join();

    Ok(())
}

fn start_process(
    info: &ProcessInfo,
    log_file: &Arc<Mutex<File>>,
) -> io::Result<Child> {
    // Form startup message
    let args_str = info.args.join(" ");
    let start_msg = format!("Starting process: {} {}", info.name, args_str);
    write_log(log_file, &start_msg)?;

    // Start process
    let mut command = Command::new(&info.name);
    if !info.args.is_empty() {
        command.args(&info.args);
    }
    
    // Configure standard streams
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    let child = command.spawn()?;

    let pid_msg = format!("Process started, PID: {}", child.id());
    write_log(log_file, &pid_msg)?;

    Ok(child)
}

fn write_log(file: &Arc<Mutex<File>>, message: &str) -> io::Result<()> {
    // Get current time
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs();

    // Format timestamp (simplified version)
    let hours = (now % 86400) / 3600;
    let minutes = (now % 3600) / 60;
    let seconds = now % 60;

    // Create log message
    let log_message = format!(
        "[2023-10-15 {:02}:{:02}:{:02}] {}\n",
        hours, minutes, seconds, message
    );

    // Write to file
    let mut file_guard = file.lock().unwrap();
    file_guard.write_all(log_message.as_bytes())?;
    
    // Also print to console
    print!("{}", log_message);

    Ok(())
}

fn command_listener(running: Arc<AtomicBool>, command_channel: Arc<CommandChannel>) {
    while running.load(Ordering::Acquire) {
        // Check for commands in the channel
        if let Some(cmd) = command_channel.get_command() {
            match cmd {
                SupervisorCommand::Leave => {
                    running.store(false, Ordering::Release);
                    break;
                }
            }
        }

        // Pause between checks
        thread::sleep(Duration::from_millis(100));
    }
}
