//! Human-friendly ctl.v1 client.  It is intentionally a thin one-call socket
//! wrapper, so agents and shell users exercise exactly the session protocol.

use std::{env, path::PathBuf, process};

use termdeck::ctl::{self, Request, Response, SCHEMA};

fn main() {
    process::exit(run(env::args().skip(1).collect()));
}

fn run(arguments: Vec<String>) -> i32 {
    let (json, socket, request) = match parse(arguments) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("termctl: {message}");
            return 2;
        }
    };
    match ctl::call(&socket, &request) {
        Ok(response) => print_response(response, json),
        Err(message) => {
            eprintln!("termctl: {message}");
            1
        }
    }
}

fn parse(arguments: Vec<String>) -> Result<(bool, PathBuf, Request), String> {
    let mut json = false;
    let mut socket = None;
    let mut requested_lines = None;
    let mut positional = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--json" => json = true,
            "--socket" => {
                socket = Some(PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "--socket requires a path".to_owned())?,
                ));
            }
            "--lines" => {
                if requested_lines.is_some() {
                    return Err("--lines can only be specified once".to_owned());
                }
                requested_lines = Some(
                    arguments
                        .next()
                        .ok_or_else(|| "--lines requires a number".to_owned())?
                        .parse::<usize>()
                        .map_err(|_| "peek lines must be a number".to_owned())?,
                );
            }
            argument if argument.starts_with('-') => {
                return Err(format!("unknown option: {argument}"));
            }
            _ => positional.push(argument),
        }
    }
    let Some(verb) = positional.first() else {
        return Err(usage().to_owned());
    };
    let (id, lines, msg) = match verb.as_str() {
        "status" | "list" | "version" if positional.len() == 1 => (None, None, None),
        "peek" if positional.len() == 2 => (Some(positional[1].clone()), requested_lines, None),
        "peek" if positional.len() == 3 && requested_lines.is_none() => (
            Some(positional[1].clone()),
            Some(
                positional[2]
                    .parse::<usize>()
                    .map_err(|_| "peek lines must be a number".to_owned())?,
            ),
            None,
        ),
        "notify" if positional.len() >= 2 => (None, None, Some(positional[1..].join(" "))),
        "status" | "list" | "peek" | "notify" | "version" => return Err(usage().to_owned()),
        _ => return Err(format!("unknown verb: {verb}")),
    };
    Ok((
        json,
        socket
            .map(Ok)
            .unwrap_or_else(ctl::socket_from_environment)?,
        Request {
            schema: SCHEMA.to_owned(),
            verb: verb.clone(),
            id,
            lines,
            msg,
        },
    ))
}

fn print_response(response: Response, json: bool) -> i32 {
    if json {
        match serde_json::to_string(&response) {
            Ok(response) => println!("{response}"),
            Err(error) => {
                eprintln!("termctl: {error}");
                return 1;
            }
        }
    } else if response.ok {
        print_plain(response.data.as_ref().unwrap_or(&serde_json::Value::Null));
    } else if let Some(error) = &response.error {
        eprintln!("termctl: {}", error.message);
    }
    response.error.map_or(0, |error| i32::from(error.code))
}

fn print_plain(data: &serde_json::Value) {
    if let Some(lines) = data.get("lines").and_then(serde_json::Value::as_array) {
        for line in lines {
            println!("{}", line.as_str().unwrap_or_default());
        }
    } else if let Some(items) = data.as_array() {
        for item in items {
            println!(
                "{}\t{}\t{}",
                item["id"].as_str().unwrap_or_default(),
                item["state"].as_str().unwrap_or_default(),
                item["path"].as_str().unwrap_or_default(),
            );
        }
    } else {
        println!("{data}");
    }
}

const fn usage() -> &'static str {
    "usage: termctl [--json] [--socket PATH] status|list|peek ID [--lines N|N]|notify MSG|version"
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parse_peek_and_notify() {
        let (_, _, peek) = parse(vec![
            "--socket".to_owned(),
            "/tmp/ctl.sock".to_owned(),
            "peek".to_owned(),
            "one".to_owned(),
            "5".to_owned(),
        ])
        .unwrap();
        assert_eq!(peek.id.as_deref(), Some("one"));
        assert_eq!(peek.lines, Some(5));

        let (_, _, peek) = parse(vec![
            "--socket".to_owned(),
            "/tmp/ctl.sock".to_owned(),
            "peek".to_owned(),
            "one".to_owned(),
            "--lines".to_owned(),
            "3".to_owned(),
        ])
        .unwrap();
        assert_eq!(peek.lines, Some(3));

        let (_, _, notify) = parse(vec![
            "--socket".to_owned(),
            "/tmp/ctl.sock".to_owned(),
            "notify".to_owned(),
            "build".to_owned(),
            "done".to_owned(),
        ])
        .unwrap();
        assert_eq!(notify.msg.as_deref(), Some("build done"));
    }
}
