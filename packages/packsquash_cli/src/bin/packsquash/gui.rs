use std::{
	collections::hash_map::RandomState,
	env, fs,
	hash::{BuildHasher, Hasher},
	io::{BufRead, BufReader, Read, Write},
	net::{TcpListener, TcpStream},
	process::{self, Child, Command, Stdio},
	sync::{Arc, Mutex, MutexGuard, PoisonError},
	thread,
	time::{Duration, Instant}
};

const PAGE: &str = include_str!("gui.html");
const PORT: u16 = 47821;
const MAX_BODY: usize = 1024 * 1024;
const IDLE_SECONDS: u64 = 40;

#[derive(Default)]
struct State {
	log: String,
	child: Option<Child>,
	running: bool,
	cli: bool,
	last_seen: Option<Instant>
}

type Shared = Arc<Mutex<State>>;

struct Request {
	method: String,
	path: String,
	query: String,
	host: String,
	body: Vec<u8>
}

struct Response {
	status: &'static str,
	content_type: &'static str,
	headers: Vec<(&'static str, String)>,
	body: Vec<u8>
}

impl Response {
	fn text(status: &'static str, body: &str) -> Self {
		Self {
			status,
			content_type: "text/plain; charset=utf-8",
			headers: Vec::new(),
			body: body.as_bytes().to_vec()
		}
	}
}

fn lock(state: &Shared) -> MutexGuard<'_, State> {
	state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn read_request(reader: impl Read) -> Option<Request> {
	let mut reader = BufReader::new(reader);
	let mut line = String::new();

	reader.read_line(&mut line).ok()?;

	let mut parts = line.split_whitespace();
	let method = parts.next()?.to_string();
	let target = parts.next()?;
	let (path, query) = target.split_once('?').unwrap_or((target, ""));
	let (path, query) = (path.to_string(), query.to_string());
	let (mut host, mut length) = (String::new(), 0_usize);

	loop {
		let mut header = String::new();

		reader.read_line(&mut header).ok()?;

		let header = header.trim_end();

		if header.is_empty() {
			break;
		}

		if let Some((name, value)) = header.split_once(':') {
			match name.to_ascii_lowercase().as_str() {
				"host" => host = value.trim().to_string(),
				"content-length" => length = value.trim().parse().ok()?,
				_ => {}
			}
		}
	}

	if length > MAX_BODY {
		return None;
	}

	let mut body = vec![0; length];

	reader.read_exact(&mut body).ok()?;

	Some(Request {
		method,
		path,
		query,
		host,
		body
	})
}

fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
	query.split('&').find_map(|pair| {
		pair.split_once('=')
			.filter(|(name, _)| *name == key)
			.map(|(_, value)| value)
	})
}

fn start(state: &Shared, options: &str) -> Result<(), String> {
	let mut guard = lock(state);

	if guard.running {
		return Err("PackSquash is already running".into());
	}

	let path = env::temp_dir().join(format!("packsquash-window-{}.toml", process::id()));

	fs::write(&path, options).map_err(|error| format!("Couldn't write the options: {error}"))?;

	let mut child = Command::new(env::current_exe().map_err(|error| error.to_string())?)
		.args(["--no-color", "--no-emoji"])
		.arg(&path)
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|error| format!("Couldn't start PackSquash: {error}"))?;

	let mut readers = Vec::new();

	if let Some(pipe) = child.stdout.take() {
		readers.push(pump(pipe, state.clone()));
	}

	if let Some(pipe) = child.stderr.take() {
		readers.push(pump(pipe, state.clone()));
	}

	guard.log.clear();
	guard.running = true;
	guard.child = Some(child);
	drop(guard);

	let state = state.clone();

	thread::spawn(move || {
		let code = loop {
			thread::sleep(Duration::from_millis(200));

			let mut guard = lock(&state);

			match guard.child.as_mut().map(Child::try_wait) {
				Some(Ok(Some(status))) => break status.code().unwrap_or(-1),
				Some(Ok(None)) => {}
				_ => break -1
			}
		};

		for reader in readers {
			let _ = reader.join();
		}

		let mut guard = lock(&state);

		guard
			.log
			.push_str(&format!("\n[PackSquash ended with the code {code}]\n"));
		guard.child = None;
		guard.running = false;
	});

	Ok(())
}

fn pump(pipe: impl Read + Send + 'static, state: Shared) -> thread::JoinHandle<()> {
	thread::spawn(move || {
		for line in BufReader::new(pipe).lines().map_while(Result::ok) {
			let mut guard = lock(&state);

			let _ = writeln!(std::io::stdout(), "{line}");
			guard.log.push_str(&line);
			guard.log.push('\n');
		}
	})
}

fn route(state: &Shared, token: &str, port: u16, request: &Request) -> Response {
	if request.host != format!("127.0.0.1:{port}") && request.host != format!("localhost:{port}") {
		return Response::text("403 Forbidden", "Open the address that PackSquash printed");
	}

	if query_value(&request.query, "t") != Some(token) {
		return Response::text("403 Forbidden", "Open the address that PackSquash printed");
	}

	let mut guard = lock(state);

	if guard.cli {
		return Response::text(
			"410 Gone",
			"The window is closed. PackSquash goes on in the terminal"
		);
	}

	guard.last_seen = Some(Instant::now());
	drop(guard);

	match (request.method.as_str(), request.path.as_str()) {
		("GET", "/") => Response {
			status: "200 OK",
			content_type: "text/html; charset=utf-8",
			headers: Vec::new(),
			body: PAGE.as_bytes().to_vec()
		},
		("GET", "/ping") => Response::text("200 OK", "ok"),
		("POST", "/run") => match start(state, &String::from_utf8_lossy(&request.body)) {
			Ok(()) => Response::text("200 OK", "started"),
			Err(message) => Response::text("409 Conflict", &message)
		},
		("POST", "/cli") => {
			let mut guard = lock(state);

			if guard.running {
				return Response::text("409 Conflict", "PackSquash is running. Stop it first");
			}

			guard.cli = true;

			Response::text("200 OK", "cli")
		}
		("POST", "/stop") => {
			if let Some(child) = lock(state).child.as_mut() {
				let _ = child.kill();
			}

			Response::text("200 OK", "stopped")
		}
		("GET", "/log") => {
			let from = query_value(&request.query, "from")
				.and_then(|from| from.parse().ok())
				.unwrap_or(0);
			let guard = lock(state);
			let mut response = Response::text(
				"200 OK",
				guard.log.get(from.min(guard.log.len())..).unwrap_or("")
			);

			response
				.headers
				.push(("X-Total", guard.log.len().to_string()));
			response
				.headers
				.push(("X-Running", u8::from(guard.running).to_string()));
			response
		}
		_ => Response::text("404 Not Found", "Not found")
	}
}

fn handle(mut stream: TcpStream, state: &Shared, token: &str, port: u16) {
	let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));

	let Some(request) = stream.try_clone().ok().and_then(read_request) else {
		return;
	};
	let response = route(state, token, port, &request);
	let mut head = format!(
		"HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Expose-Headers: X-Total, X-Running\r\nConnection: close\r\n",
		response.status,
		response.content_type,
		response.body.len()
	);

	for (name, value) in &response.headers {
		head.push_str(&format!("{name}: {value}\r\n"));
	}

	head.push_str("\r\n");

	let _ = stream
		.write_all(head.as_bytes())
		.and_then(|()| stream.write_all(&response.body));
}

fn random_token() -> String {
	let mut first = RandomState::new().build_hasher();
	let mut second = RandomState::new().build_hasher();

	first.write_u32(process::id());
	second.write_u128(
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map_or(0, |elapsed| elapsed.as_nanos())
	);

	format!("{:016x}{:016x}", first.finish(), second.finish())
}

fn open_window(url: &str) {
	if env::var_os("PACKSQUASH_NO_BROWSER").is_some() {
		return;
	}

	let run = |program: &str, args: &[&str]| {
		Command::new(program)
			.args(args)
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.status()
			.is_ok_and(|status| status.success())
	};

	if cfg!(windows) {
		let _ = run(
			"cmd",
			&["/C", "start", "", "msedge", &format!("--app={url}")]
		) || run("cmd", &["/C", "start", "", url]);
	} else if cfg!(target_os = "macos") {
		let _ = run("open", &[url]);
	} else {
		let _ = run("xdg-open", &[url]);
	}
}

#[cfg(windows)]
pub fn opened_by_double_click() -> bool {
	use windows_sys::Win32::System::Console::GetConsoleProcessList;

	let mut processes = [0u32; 2];

	#[allow(unsafe_code)] // SAFETY: the buffer holds the two process identifiers that the length says
	let attached_processes = unsafe { GetConsoleProcessList(processes.as_mut_ptr(), 2) };

	attached_processes == 1
}

#[cfg(not(windows))]
pub fn opened_by_double_click() -> bool {
	false
}

pub fn run() -> Option<i32> {
	let listener = match TcpListener::bind(("127.0.0.1", PORT))
		.or_else(|_| TcpListener::bind(("127.0.0.1", 0)))
	{
		Ok(listener) => listener,
		Err(error) => {
			eprintln!("Couldn't open the window: {error}");

			return Some(1);
		}
	};
	let port = listener.local_addr().map_or(PORT, |address| address.port());
	let token = random_token();
	let url = format!("http://127.0.0.1:{port}/?t={token}");
	let idle = env::var("PACKSQUASH_GUI_IDLE")
		.ok()
		.and_then(|seconds| seconds.parse().ok())
		.unwrap_or(IDLE_SECONDS);
	let state = Shared::default();

	lock(&state).last_seen = Some(Instant::now());

	println!("The PackSquash window is at {url}");
	println!("Close the window to end. Press Ctrl+C to end now.");
	open_window(&url);

	let server = state.clone();
	let server_token = token.clone();

	thread::spawn(move || {
		for stream in listener.incoming().flatten() {
			let state = server.clone();
			let token = server_token.clone();

			thread::spawn(move || handle(stream, &state, &token, port));
		}
	});

	loop {
		thread::sleep(Duration::from_millis(200));

		let guard = lock(&state);

		if guard.cli {
			return None;
		}

		if !guard.running
			&& guard
				.last_seen
				.is_some_and(|seen| seen.elapsed() > Duration::from_secs(idle))
		{
			println!("The window was closed.");
			process::exit(0);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn request(text: &str) -> Request {
		read_request(text.as_bytes()).unwrap()
	}

	#[test]
	fn a_request_is_read_with_its_query_host_and_body() {
		let request = request(
			"POST /run?t=abc&from=3 HTTP/1.1\r\nHost: 127.0.0.1:1\r\nContent-Length: 5\r\n\r\nhello"
		);

		assert_eq!(request.method, "POST");
		assert_eq!(request.path, "/run");
		assert_eq!(query_value(&request.query, "t"), Some("abc"));
		assert_eq!(query_value(&request.query, "from"), Some("3"));
		assert_eq!(request.host, "127.0.0.1:1");
		assert_eq!(request.body, b"hello");
		assert!(
			read_request(&b"POST /run HTTP/1.1\r\nContent-Length: 99999999\r\n\r\n"[..]).is_none()
		);
		assert!(read_request(&b""[..]).is_none());
	}

	#[test]
	fn only_the_right_token_and_host_get_in() {
		let state = Shared::default();
		let get = |target: &str, host: &str| {
			route(
				&state,
				"secret",
				5,
				&request(&format!("GET {target} HTTP/1.1\r\nHost: {host}\r\n\r\n"))
			)
			.status
		};

		assert_eq!(get("/?t=secret", "127.0.0.1:5"), "200 OK");
		assert_eq!(get("/?t=secret", "localhost:5"), "200 OK");
		assert_eq!(get("/?t=wrong", "127.0.0.1:5"), "403 Forbidden");
		assert_eq!(get("/", "127.0.0.1:5"), "403 Forbidden");
		assert_eq!(get("/?t=secret", "evil.example:5"), "403 Forbidden");
		assert_eq!(get("/nothing?t=secret", "127.0.0.1:5"), "404 Not Found");
		assert_eq!(get("/ping?t=secret", "127.0.0.1:5"), "200 OK");
	}

	#[test]
	fn the_log_is_read_from_an_offset() {
		let state = Shared::default();

		lock(&state).log.push_str("one\ntwo\n");

		let log = |from: usize| {
			route(
				&state,
				"t",
				5,
				&request(&format!(
					"GET /log?t=t&from={from} HTTP/1.1\r\nHost: 127.0.0.1:5\r\n\r\n"
				))
			)
		};

		assert_eq!(log(0).body, b"one\ntwo\n");
		assert_eq!(log(4).body, b"two\n");
		assert_eq!(log(99).body, b"");
		assert!(log(0).headers.contains(&("X-Total", "8".to_string())));
		assert!(log(0).headers.contains(&("X-Running", "0".to_string())));
	}

	#[test]
	fn the_cli_button_ends_the_window() {
		let state = Shared::default();
		let call = |method: &str| {
			route(
				&state,
				"t",
				5,
				&request(&format!(
					"{method} /cli?t=t HTTP/1.1\r\nHost: 127.0.0.1:5\r\n\r\n"
				))
			)
		};

		assert_eq!(call("POST").status, "200 OK");
		assert!(lock(&state).cli);
		assert_eq!(call("GET").status, "410 Gone");
	}

	#[test]
	fn the_page_has_what_the_server_needs() {
		for needed in [
			"id=\"run\"",
			"id=\"stop\"",
			"id=\"cli\"",
			"/cli",
			"id=\"log\"",
			"/ping",
			"/run",
			"/log"
		] {
			assert!(PAGE.contains(needed), "{needed}");
		}
	}
}
