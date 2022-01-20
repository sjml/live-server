use async_std::prelude::*;
use colored::Colorize;
use html_editor::{parse, Editable, Htmlifiable, Node, Selector};
use local_ip_address::local_ip;
use once_cell::sync::OnceCell;
use std::fs;
use tide::{listener::Listener, Request, Response, StatusCode};
use tide_websockets::WebSocket;
use uuid::Uuid;

use crate::{PORT, WS_CLIENTS};

pub static SCRIPT: OnceCell<Node> = OnceCell::new();

pub async fn serve() {
    let host = local_ip().unwrap().to_string();
    let mut port = PORT.get().unwrap().clone();
    let mut listener = create_listener(&host, &mut port).await;

    init_ws_script();

    let url = format!("http://{}:{}/", host, port);
    println!(" Server listening on {}", url.blue());
    listener.accept().await.unwrap();
}

fn create_server() -> tide::Server<()> {
    let mut app = tide::new();
    app.at("/").get(static_assets);
    app.at("/*").get(static_assets);
    app.at("/live-server-ws")
        .get(WebSocket::new(|_request, mut stream| async move {
            let uuid = Uuid::new_v4();
            // Add the connection to clients when opening a new connection
            WS_CLIENTS.lock().await.insert(uuid, stream.clone());
            // Waiting for the connection to be closed
            while let Some(Ok(_)) = stream.next().await {}
            // Remove the connection from clients when it is closed
            WS_CLIENTS.lock().await.remove(&uuid);
            Ok(())
        }));
    app
}

async fn create_listener(host: &String, port: &mut u16) -> impl Listener<()> {
    loop {
        let app = create_server();
        match app.bind(format!("{}:{}", host, port)).await {
            Ok(listener) => break listener,
            Err(err) => {
                if let std::io::ErrorKind::AddrInUse = err.kind() {
                    let info = format!("[WARNING] Port {} is already in use", port);
                    println!("{}", info.yellow());
                    *port += 1;
                } else {
                    let info = format!(
                        "[ERROR] Failed to bind port to {}: {}",
                        port,
                        err.to_string()
                    );
                    panic!("{}", info.red());
                }
            }
        }
    }
}

fn init_ws_script() {
    SCRIPT
        .set({
            let script = format!(
                r#"
                    const ws = new WebSocket("ws://{}:{}/live-server-ws");
                    ws.onopen = () => console.log("[Live Server] Connection Established");
                    ws.onmessage = () => location.reload();
                    ws.onclose = () => console.log("[Live Server] Connection Closed");
                "#,
                local_ip().unwrap(),
                PORT.get().unwrap()
            );
            Node::new_element("script", vec![], vec![Node::Text(script)])
        })
        .unwrap();
}

async fn static_assets(req: Request<()>) -> tide::Result {
    // Get the path and mime of the static file.
    let mut path = req.url().path().to_string();
    path = if path.ends_with("/") {
        format!(".{}index.html", path)
    } else {
        format!(".{}", path)
    };
    let mime = mime_guess::from_path(&path).first_or_text_plain();

    // Read the file.
    let file = match fs::read(&path) {
        Ok(file) => file,
        Err(err) => {
            let info = format!(r#"[ERROR] Failed to read "{}": {}"#, path, err.to_string());
            eprintln!("{}", info.red());
            return Err(tide::Error::new(StatusCode::NotFound, err));
        }
    };
    let mut file: String = String::from_utf8_lossy(&file).parse()?;

    // Construct the response.
    let mut response: Response;
    if mime == "text/html" {
        let head_selector = Selector::from("head");
        let script = SCRIPT.get().unwrap().clone();
        file = parse(file.as_str())
            .insert_to(&head_selector, script)
            .html();
    }
    response = file.into();
    response.set_content_type(mime.to_string().as_str());

    Ok(response)
}
