//! Standalone console demonstration of the state/listener/data-flow foundation.
//! `+` or `-` changes count, `s` changes step, `q` exits. One model feeds two mounts.
//! Copy this file into a binary depending on rxui; no native/GPU setup is required.
use rxui::{Entity, Listener, Mount, Runtime};
use std::{
    error::Error,
    io::{self, Write},
};

struct Counter {
    value: i32,
    step: i32,
}
struct Controls {
    caption: String,
    edit: Listener<i32>,
}

fn controls(runtime: &mut Runtime, mount: &Mount<Counter>) -> Result<Controls, rxui::AccessError> {
    runtime.evaluate(mount, |state, cx| Controls {
        caption: format!("Counter: {} (step {})", state.value, state.step),
        edit: cx.listener(|this, direction: &i32, _cx| this.value += this.step * direction),
    })
}
fn status(
    runtime: &mut Runtime,
    mount: &Mount<()>,
    count: &Entity<Counter>,
) -> Result<String, rxui::AccessError> {
    runtime.evaluate(mount, |_, cx| {
        format!("Shared status: {}", count.read(cx).value)
    })
}
fn main() -> Result<(), Box<dyn Error>> {
    let mut runtime = Runtime::new();
    let (counter, control_mount, status_mount) = runtime.update(|cx| {
        let counter = cx.new(|_| Counter { value: 0, step: 1 });
        let status = cx.new(|_| ());
        let controls = cx.mount(&counter).unwrap();
        let status = cx.mount(&status).unwrap();
        (counter, controls, status)
    });
    let mut view = controls(&mut runtime, &control_mount)?;
    println!(
        "{}\n{}",
        view.caption,
        status(&mut runtime, &status_mount, &counter)?
    );
    println!("Commands: +, -, s (toggle step), q");
    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        match line.trim() {
            "+" => {
                runtime.update(|cx| view.edit.dispatch(&1, cx))?;
            }
            "-" => {
                runtime.update(|cx| view.edit.dispatch(&-1, cx))?;
            }
            "s" => runtime.update(|cx| {
                counter.update(cx, |this, _| {
                    this.step = if this.step == 1 { 10 } else { 1 }
                })
            }),
            "q" => break,
            _ => {
                println!("Use +, -, s, or q.");
                continue;
            }
        }
        runtime.flush()?;
        if runtime.is_dirty(&control_mount)? {
            view = controls(&mut runtime, &control_mount)?;
            println!("{}", view.caption);
        }
        if runtime.is_dirty(&status_mount)? {
            println!("{}", status(&mut runtime, &status_mount, &counter)?);
        }
    }
    Ok(())
}
