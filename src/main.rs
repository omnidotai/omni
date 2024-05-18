#![allow(unreachable_code)]

use inquire::Text;
use reqwest::Client;
use serde_json::{self, json, Value};
use std::{
    env,
    fs::OpenOptions,
    future::Future,
    io::{Read, Write},
    path::Path,
    pin::Pin,
};

use tokio::process::Command;

use termimad::MadSkin;

#[cfg(target_os = "windows")]
const SHELL: &str = "cmd.exe";
#[cfg(target_os = "windows")]
const FLAG: &str = "/C";

#[cfg(not(target_os = "windows"))]
const SHELL: &str = "/bin/sh";
#[cfg(not(target_os = "windows"))]
const FLAG: &str = "-c";

type OResult<T> = Result<T, Box<dyn std::error::Error>>;
static mut FILE: Option<Box<String>> = None;

macro_rules! func {
    (fn $func_name:tt [$desc:tt] ($($a:tt [$adesc:expr]),*) * [$($req:expr),*]) => {
        json!({
            "type": "function",
            "function": {
                "name": $func_name,
                "desciption": $desc,
                "parameters": {
                    "type": "object",
                    "properties": {
                        $(
                            $a: {
                                "type": "string",
                                "description": $adesc
                            }
                        ),*
                    },
                    "required": [$($req),*]
                }
            }
        })
    };
}

macro_rules! allfn {
    [$(fn $func_name:tt [$desc:tt] ($($a:tt [$adesc:expr]),*) * [$($req:expr),*]),*] => {
        json!([
            $(
                func!(fn $func_name [$desc] ($($a [$desc]),*) * [$($req),*])
            ),*
        ])
    };
}

pub fn get_tools() -> Value {
    allfn![
        /*
        fn "browse" ["search through the internet (via Google)"]
            (
                "query" ["the term to search for"]
            ) * ["query"]
            ,
        */
        fn "exec" ["execute shell commands"]
            (
                "cmd" ["the command to run"],
                "reason" ["why do you want to run it?"]
            ) * ["cmd", "reason"]
    ]
}

pub async fn exec(cmd: &Value, reason: &Value) -> OResult<Value> {
    let confirm = inquire::Confirm::new(format!("Do you want to run {cmd}?").as_str())
        .with_default(false)
        .with_help_message(format!("reason: {reason}").as_str())
        .prompt()?;
    if confirm {
        let o = Command::new(SHELL)
            .arg(FLAG)
            .arg(cmd.as_str().unwrap())
            .output();
        let o = o.await?;
        let stdout = std::str::from_utf8(o.stdout.as_slice())
            .unwrap_or("failed to decode as UTF-8 (perhaps STDOUT isn't String?)");
        let stderr = std::str::from_utf8(o.stderr.as_slice())
            .unwrap_or("failed to decode as UTF-8 (perhaps STDERR isn't String?)");
        let status = o.status.code().unwrap();
        Ok(json!({
            "stdout": stdout,
            "stderr": stderr,
            "status_code": status
        }))
    } else {
        Ok(json!({
            "error": "Sorry, Omni! Your request to run the command was denied! Perhaps, you should be confirmed first?"
        }))
    }
}

pub async fn search(q: &Value) -> OResult<Value> {
    match inquire::Confirm::new(format!("Do you want to search {q} on Google?").as_str())
        .with_default(false)
        .with_help_message("only allow if you're sure (although still harmless otherwise)")
        .prompt()?
    {
        true => Ok(json!({
            "query": q,
            "results": "unimplemented!"
        })),
        false => Ok(json!({
            "error": "Sorry, Omni! Your request to search something up was denied! Perhaps, rethink your choice?"
        })),
    }
}

pub async fn call_tool(tool: &Value, args: &Value) -> OResult<Value> {
    let r = match tool.as_str().unwrap() {
        "exec" => exec(&args["cmd"], &args["reason"]).await?,
        "browse" => search(&args["query"]).await?,
        _ => todo!(),
    };
    Ok(json!(r.to_string()))
}

pub fn load_file(path: &Path, sys_prompt: &mut Value) -> OResult<()> {
    let p = path.canonicalize()?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&p)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    if contents.as_str() == "" {
        contents = "[]".to_string();
    }
    let parsed: Value = serde_json::from_str(contents.as_str())?;
    sys_prompt["messages"] = parsed;
    unsafe { FILE = Some(Box::new(p.to_str().unwrap().to_string())) };
    Ok(())
}

pub fn export(sys_prompt: &mut Value, path: &Path) -> OResult<()> {
    {
        let arr = sys_prompt["messages"].as_array_mut().unwrap();
        if arr.len() == 0 {
            arr.push(json!({
                "role": "system",
                "content": "You're a multi-modal, multi-featured, multi-powered, multi-client AI bot, who can do next to everything. Really not, you can only do normal text stuff, and, the best part, tool call. You have tools for everything, including calling other AIs (like Stability for Image etc.), searching the web and other stuff! You shall refer to them as Capabilities. Remember, you're Omni.AI, and also, don't hurry to use tools. Use them consciously, and only when think they should be used."
            }))
        }
    }
    let p = path.canonicalize();
    let confirm = inquire::Confirm::new(
        format!(
            "Do you really want to export the conversations to `{}`?",
            &path.to_str().unwrap()
        )
        .as_str(),
    )
    .with_help_message("Note: it'll remove any existing content of the file!")
    .with_default(false)
    .prompt();
    match confirm {
        Ok(true) => {
            let mut f = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .open(&path)?;
            f.write_all(sys_prompt["messages"].to_string().as_bytes())?;
            unsafe {
                FILE = Some(Box::new(p?.to_str().unwrap().to_string()));
            }
            println!("Exported successfully!");
        }
        Ok(false) => {
            println!("Aborting export!");
        }
        Err(e) => {
            eprintln!("Error occured: {e}");
        }
    }
    Ok(())
}

pub async fn create_completion(
    sys_prompt: &mut Value,
    prompt: String,
    client: &mut Client,
    token: &String,
    skin: &MadSkin,
) -> OResult<()> {
    // Use create_completion_indiv, after doing the processing
    if sys_prompt["messages"].as_array().unwrap().len() == 0 {
        {
            let sys = json!({
                "content": "You're a multi-modal, multi-featured, multi-powered, multi-client AI bot, who can do next to everything. Really not, you can only do normal text stuff, and, the best part, tool call. You have tools for everything, including calling other AIs (like Stability for Image etc.), searching the web and other stuff! You shall refer to them as Capabilities. Remember, you're Omni.AI, and also, don't hurry to use tools. Use them consciously, and only when you think they should be used.",
                "role": "system"
            });
            sys_prompt["messages"].as_array_mut().unwrap().push(sys)
        }
    }
    {
        sys_prompt["messages"].as_array_mut().unwrap().push(json!({
            "role": "user",
            "content": prompt
        }));
    }
    create_completion_indiv(sys_prompt, client, token).await?;
    skin.print_text(
        sys_prompt["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    );
    Ok(())
}

// create a completion, doesn't take care of anything else. just returns the resulting Value
pub async fn create_completion_indiv(
    all: &mut Value,
    client: &mut Client,
    token: &String,
) -> OResult<()> {
    let res = client
        .post("https://api.groq.com/openai/v1/chat/completions")
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .body(all.to_string())
        .send()
        .await?;
    let body: Value = serde_json::from_str(res.text().await?.as_str())?;
    let msg = &body["choices"][0]["message"];
    {
        all["messages"].as_array_mut().unwrap().push(msg.clone());
    }
    let tool_calls = &msg["tool_calls"];
    if tool_calls != &Value::Null {
        // tool is called. handle it.
        for call in tool_calls.as_array().unwrap() {
            let f = &call["function"];
            let name = &f["name"];
            let id = &call["id"];
            let args = &f["arguments"];
            // parse args as JSON
            let pargs = serde_json::from_str::<Value>(args.as_str().unwrap());
            if let Ok(a) = pargs {
                // proper JSON args
                let r = &call_tool(name, &a).await?;
                {
                    all["messages"].as_array_mut().unwrap().push(json!({
                        "role": "tool",
                        "tool_call_id": id,
                        "name": name,
                        "content": r
                    }));
                }
            }
        }
        // tool processing done. time for re-doing the req.
        let result: Pin<Box<dyn Future<Output = OResult<()>>>> =
            Box::pin(create_completion_indiv(all, client, token));
        result.await?;
        return Ok(());
    }
    if let Some(p) = unsafe { FILE.clone() } {
        let p = Path::new(&*p.as_str());
        let mut f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(p)?;
        f.write_all(all["messages"].to_string().as_bytes())?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> OResult<()> {
    let mut sys_prompt = json!(
        {
            "messages": [],
            "model": "llama3-70b-8192",
            "tool_choice": "auto",
            "tools": get_tools(),
        }
    );
    dotenv::dotenv()?;
    let token = env::var("GROQ_TOKEN");
    let skin = MadSkin::default();
    if let Ok(token) = token {
        let mut client = Client::builder()
            .user_agent("Omni.AI/1.0 (Rust; reqwest)")
            .build()?;
        loop {
            let prompt = Text::new("")
                .with_placeholder("enter text or /q[uit] to exit, or /load <file.json> to load/store from/to JSON.")
                .prompt();
            match prompt {
                Ok(p) => {
                    match p.as_str() {
                        "/q" | "/quit" => {
                            std::process::exit(0);
                        }
                        p if p.starts_with("/load") => {
                            let p = p.split(" ").nth(1).unwrap();
                            load_file(Path::new(p), &mut sys_prompt)?;
                        }
                        p if p.starts_with("/export") => {
                            let p = p.split(" ").nth(1).unwrap();
                            export(&mut sys_prompt, Path::new(p))?;
                        }
                        _ => {
                            create_completion(&mut sys_prompt, p, &mut client, &token, &skin)
                                .await?;
                        }
                    };
                }
                Err(e) => {
                    eprintln!("Error occured: {e}")
                }
            }
        }
    } else {
        panic!("GROQ_TOKEN isn't set!");
    }
    Ok(())
}
