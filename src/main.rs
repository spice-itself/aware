use chrono::Local;
// Импорты nix теперь должны работать с включенными features
use nix::{
    sys::signal::{kill, Signal},
    unistd::Pid,
};
use signal_hook::{consts::SIGTERM, flag as signal_flag};
use std::{
    // Добавляем env обратно, так как main будет восстановлена
    env,
    fs::{self, File, OpenOptions},
    // Убираем неиспользуемый Read
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

// --- ВОССТАНАВЛИВАЕМ ОПРЕДЕЛЕНИЕ ProcessInfo ---
struct ProcessInfo {
    name: String,          // Полный путь или имя программы
    program_name: String,  // Имя файла программы (для имен файлов)
    args: Vec<String>,
    log_path: PathBuf,     // Используем PathBuf для путей
    pid_path: PathBuf,
}
// -----------------------------------------------

// --- ВОССТАНАВЛИВАЕМ main ---
fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    // Создаем директории заранее, если их нет
    fs::create_dir_all("aware_logs")?;
    fs::create_dir_all("aware_pids")?;

    match args[1].as_str() {
        "supervise" => {
            if args.len() < 3 {
                eprintln!("Ошибка: Укажите программу для запуска."); // Пишем ошибки в stderr
                print_usage();
                return Ok(());
            }

            let program_path_str = &args[2];
            let program_path = Path::new(program_path_str);

            // Получаем имя программы из пути
            let program_name = program_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| program_path_str.to_string()); // Запасной вариант, если имя извлечь не удалось

            // Пути к лог-файлу и PID-файлу
            let log_path = PathBuf::from(format!("aware_logs/{}.log", program_name));
            let pid_path = PathBuf::from(format!("aware_pids/{}.pid", program_name));

            // Информация о процессе
            let process_info = ProcessInfo {
                name: program_path_str.clone(),
                program_name: program_name.clone(), // Сохраняем чистое имя
                args: args[3..].to_vec(),
                log_path,
                pid_path: pid_path.clone(), // Клонируем для передачи
            };

            // Записываем PID-файл (с PID текущего супервизора)
            // Передаем &pid_path вместо строки
            write_pid_file(&pid_path, &process_info.program_name)?;

            // Запускаем супервизор
            // Используем expect для критических ошибок
            run_supervisor(process_info).expect("Не удалось запустить супервизор");
        }
        "leave" => {
            if args.len() >= 3 {
                // Остановка конкретной программы
                send_leave_signal(Some(&args[2]))?;
            } else {
                // Остановка всех программ
                send_leave_signal(None)?;
            }
        }
        _ => {
            eprintln!("Неизвестная команда: {}", args[1]);
            eprintln!("Поддерживаемые команды: supervise, leave");
        }
    }

    Ok(())
}
// -----------------------------

// --- ВОССТАНАВЛИВАЕМ print_usage ---
fn print_usage() {
    println!(
        "Использование:\n  aware supervise <программа> [аргументы...]\n  aware leave [имя_программы]"
    );
}
// ------------------------------------

// -- Управление PID-файлами --

// Записывает PID текущего процесса (супервизора) в файл
fn write_pid_file(pid_path: &Path, program_name: &str) -> io::Result<()> {
    // Записываем PID текущего процесса
    let pid = std::process::id();
    fs::write(pid_path, pid.to_string())?; // fs::write проще для записи строки

    // Добавляем запись в общий список процессов (опционально, но полезно для leave all)
    let list_path = PathBuf::from("aware_pids/processes.list");
    let mut list_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(list_path)?;
    // Храним путь к pid файлу для простоты поиска при leave all
    writeln!(list_file, "{}:{}", program_name, pid_path.display())?;

    Ok(())
}

// --- ВОССТАНАВЛИВАЕМ cleanup_pid_file ---
// Удаляет PID-файл и запись из списка (если нужно)
fn cleanup_pid_file(pid_path: &Path, program_name: &str) -> io::Result<()> {
    if pid_path.exists() {
        fs::remove_file(pid_path)?;
        println!("PID-файл удален: {}", pid_path.display()); // Добавим лог
    }

    // Очистка из списка (более сложная, требует чтения и перезаписи)
    // TODO: Реализовать очистку processes.list при необходимости
    let _ = program_name; // Убираем warning о неиспользуемой переменной

    Ok(())
}
// ----------------------------------------

// -- Отправка сигнала для остановки --

fn send_leave_signal(program_name: Option<&str>) -> io::Result<()> {
    // --- УДАЛЯЕМ НЕИСПОЛЬЗУЕМУЮ ПЕРЕМЕННУЮ ---
    // let pids_to_signal: Vec<(String, u32)> = Vec::new();
    // ----------------------------------------

    if let Some(name) = program_name {
        // Сигнал конкретному процессу
        println!("Отправка команды leave процессу {}", name);
        let pid_path = PathBuf::from(format!("aware_pids/{}.pid", name));
        match read_pid_from_file(&pid_path) {
            Ok(pid) => {
                println!("Отправка SIGTERM процессу {} (PID: {})", name, pid);
                // Отправляем сигнал SIGTERM
                match kill(Pid::from_raw(pid as i32), Some(Signal::SIGTERM)) {
                    Ok(_) => println!("Сигнал отправлен успешно."),
                    Err(e) => eprintln!("Ошибка отправки сигнала процессу {}: {}", pid, e),
                }
            }
            Err(e) => {
                eprintln!(
                    "Не удалось прочитать PID для {}: {}. Процесс не запущен или PID-файл поврежден.",
                    name, e
                );
            }
        }
    } else {
        // Сигнал всем процессам
        println!("Отправка команды leave всем запущенным aware процессам");
        let pid_dir_path = PathBuf::from("aware_pids");
        if !pid_dir_path.exists() {
            println!("Директория PID-файлов не найдена.");
            return Ok(());
        }

        // Итерируемся по *.pid файлам
        match fs::read_dir(pid_dir_path) {
            Ok(entries) => {
                 for entry_result in entries {
                     if let Ok(entry) = entry_result {
                         let path = entry.path();
                         if path.is_file() && path.extension().map_or(false, |ext| ext == "pid") {
                             let name = path.file_stem().map_or_else(
                                || "unknown".to_string(), // Вряд ли случится
                                |stem| stem.to_string_lossy().into_owned()
                             );
                             match read_pid_from_file(&path) {
                                 Ok(pid) => {
                                     println!("Отправка SIGTERM процессу {} (PID: {})", name, pid);
                                     match kill(Pid::from_raw(pid as i32), Some(Signal::SIGTERM)) {
                                         Ok(_) => {} // Успешно
                                         Err(e) => eprintln!("Ошибка отправки сигнала процессу {}: {}", pid, e),
                                     }
                                 }
                                 Err(e) => {
                                     eprintln!("Не удалось прочитать PID для {}: {}", name, e);
                                 }
                             }
                         }
                     }
                 }
                 println!("Команда leave отправлена всем найденным процессам.");
            },
            Err(e) => eprintln!("Не удалось прочитать директорию PID-файлов: {}", e),
        }
    }
    Ok(())
}

// --- ВОССТАНАВЛИВАЕМ read_pid_from_file ---
fn read_pid_from_file(pid_path: &Path) -> io::Result<u32> {
    let pid_str = fs::read_to_string(pid_path)?;
    pid_str
        .trim()
        .parse::<u32>()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Ошибка парсинга PID: {}", e)))
}
// ----------------------------------------

// -- Основной цикл супервизора --

// --- Используем ProcessInfo ---
fn run_supervisor(info: ProcessInfo) -> io::Result<()> {
    println!(
        "Запуск супервизора для программы: {}",
        info.program_name
    );
    println!("Логи будут сохраняться в: {}", info.log_path.display());

    // Открываем файл логов с мьютексом для потокобезопасной записи
    let log_file = Arc::new(Mutex::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&info.log_path)?,
    ));

    write_log(&log_file, "Запуск супервизора")?;

    // Устанавливаем обработчик сигнала SIGTERM
    let term_signal = Arc::new(AtomicBool::new(false));
    // Регистрируем обработчик. Используем expect, так как ошибка здесь критична.
    signal_flag::register(SIGTERM, Arc::clone(&term_signal))
        .expect("Не удалось зарегистрировать обработчик сигнала SIGTERM");

    let mut child_handle: Option<Child> = None;
    let mut stdout_handle: Option<JoinHandle<()>> = None;
    let mut stderr_handle: Option<JoinHandle<()>> = None;

    // Основной цикл супервизора
    loop {
        // Проверяем, не пришел ли сигнал завершения
        if term_signal.load(Ordering::Relaxed) {
            write_log(&log_file, "Получен сигнал SIGTERM, начинаем завершение...")?;
            if let Some(mut child) = child_handle.take() {
                 write_log(&log_file, "Отправка SIGTERM дочернему процессу...")?;
                 match child.kill() { // Посылаем SIGKILL (или можно SIGTERM сначала, потом SIGKILL)
                     Ok(_) => {
                         write_log(&log_file, "Ожидание завершения дочернего процесса...")?;
                         match child.wait() {
                            Ok(status) => write_log(&log_file, &format!("Дочерний процесс завершен со статусом: {}", status))?,
                            Err(e) => write_log(&log_file, &format!("Ошибка ожидания дочернего процесса: {}", e))?,
                         }
                     },
                     Err(e) => write_log(&log_file, &format!("Ошибка отправки сигнала kill дочернему процессу: {}", e))?,
                 }
            }
             // Завершаем потоки логирования при остановке по сигналу
            if let Some(h) = stdout_handle.take() { h.join().expect("Поток stdout паниковал при завершении"); }
            if let Some(h) = stderr_handle.take() { h.join().expect("Поток stderr паниковал при завершении"); }
            break; // Выходим из основного цикла
        }

        // Запускаем процесс, если он еще не запущен или упал
        if child_handle.is_none() {
            match start_process(&info, &log_file) {
                Ok((child, h_out, h_err)) => {
                    child_handle = Some(child);
                    stdout_handle = Some(h_out);
                    stderr_handle = Some(h_err);
                }
                Err(e) => {
                    write_log(&log_file, &format!("Ошибка запуска процесса: {}. Повтор через 5 сек...", e))?;
                    thread::sleep(Duration::from_secs(5));
                    continue; // Переходим к следующей итерации для повторного запуска
                }
            };
        }

        // Проверяем состояние дочернего процесса
        // Убираем 'mut' из 'mut child', так как child.try_wait() не требует &mut self
        if let Some(child) = child_handle.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Процесс завершился
                    write_log(
                        &log_file,
                        &format!("Процесс завершился со статусом: {}", status),
                    )?;
                    child_handle = None; // Сбрасываем хэндл, чтобы перезапустить на след. итерации

                    // Ждем завершения потоков логирования
                    if let Some(h) = stdout_handle.take() { h.join().expect("Поток stdout паниковал"); }
                    if let Some(h) = stderr_handle.take() { h.join().expect("Поток stderr паниковал"); }

                    // Пауза перед перезапуском (если не было сигнала)
                     if !term_signal.load(Ordering::Relaxed) {
                         write_log(&log_file, "Перезапуск через 2 секунды...")?;
                         thread::sleep(Duration::from_secs(2));
                     }
                }
                Ok(None) => {
                    // Процесс еще работает, ничего не делаем, короткая пауза
                    thread::sleep(Duration::from_millis(200));
                }
                Err(e) => {
                    // Ошибка при проверке статуса
                    write_log(
                        &log_file,
                        &format!("Ошибка проверки статуса процесса: {}. Перезапуск...", e),
                    )?;
                    child_handle = None; // Сбрасываем для перезапуска

                     // Завершаем потоки логирования при ошибке
                     if let Some(h) = stdout_handle.take() { h.join().expect("Поток stdout паниковал"); }
                     if let Some(h) = stderr_handle.take() { h.join().expect("Поток stderr паниковал"); }

                     thread::sleep(Duration::from_secs(2)); // Пауза перед перезапуском
                }
            }
        }
    } // Конец основного цикла loop

    write_log(&log_file, "Супервизор завершает работу.")?;

    // Очистка PID-файла при завершении
    // --- Используем cleanup_pid_file ---
    cleanup_pid_file(&info.pid_path, &info.program_name)
        .map_err(|e| eprintln!("Ошибка при очистке PID-файла: {}", e))
        .ok(); // Игнорируем ошибку очистки, если она произошла

    Ok(())
}

// -- Запуск дочернего процесса и логирование его вывода --

// --- Используем &ProcessInfo ---
fn start_process(
    info: &ProcessInfo,
    log_file_arc: &Arc<Mutex<File>>,
) -> io::Result<(Child, JoinHandle<()>, JoinHandle<()>)> { // Возвращаем хендлы потоков логов
    let args_str = info.args.join(" ");
    write_log(log_file_arc,&format!("Запуск процесса: {} {}", info.name, args_str))?;

    let mut command = Command::new(&info.name);
    if !info.args.is_empty() {
        command.args(&info.args);
    }

    // Перенаправляем stdout и stderr
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn()?; // Используем mut child здесь, т.к. берем .stdout/.stderr

    let pid_msg = format!("Процесс запущен, PID: {}", child.id());
    write_log(log_file_arc, &pid_msg)?;

    // --- Запуск потоков логирования ---

    // stdout
    let stdout = child.stdout.take()
        .expect("Не удалось получить stdout дочернего процесса"); // Критическая ошибка
    let log_stdout = Arc::clone(log_file_arc);
    let stdout_handle = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line_result in reader.lines() {
            match line_result {
                // Пишем в лог с префиксом [stdout]
                Ok(line) => { let _ = write_log(&log_stdout, &format!("[stdout] {}", line)); },
                Err(e) => { let _ = write_log(&log_stdout, &format!("[stdout error] Ошибка чтения: {}", e)); break; }
            }
        }
        // Можно добавить лог о завершении потока
        let _ = write_log(&log_stdout, "[stdout thread finished]");
    });

    // stderr
    let stderr = child.stderr.take()
        .expect("Не удалось получить stderr дочернего процесса"); // Критическая ошибка
    let log_stderr = Arc::clone(log_file_arc);
    let stderr_handle = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line_result in reader.lines() {
             match line_result {
                // Пишем в лог с префиксом [stderr]
                Ok(line) => { let _ = write_log(&log_stderr, &format!("[stderr] {}", line)); },
                Err(e) => { let _ = write_log(&log_stderr, &format!("[stderr error] Ошибка чтения: {}", e)); break; }
            }
        }
         // Можно добавить лог о завершении потока
        let _ = write_log(&log_stderr, "[stderr thread finished]");
    });

    Ok((child, stdout_handle, stderr_handle))
}


// -- Утилита логирования --

fn write_log(log_file_arc: &Arc<Mutex<File>>, message: &str) -> io::Result<()> {
    // Используем chrono для форматирования времени
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let log_message = format!("[{}] {}\n", timestamp, message);

    // Запись в файл
    // Используем expect, так как отравленный мьютекс здесь критичен
    let mut file_guard = log_file_arc.lock().expect("Мьютекс лог-файла отравлен!");
    file_guard.write_all(log_message.as_bytes())?;

    // Также выводим в консоль супервизора
    print!("{}", log_message);

    Ok(())
}
