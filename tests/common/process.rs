#![allow(dead_code)]

use std::process::Command;

pub fn run_with_timeout(command: &mut Command, seconds: u64) -> Result<(), String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawning the suite: {error}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);

    loop {
        match child
            .try_wait()
            .map_err(|error| format!("waiting for the suite: {error}"))?
        {
            Some(status) if status.code().is_some() => return Ok(()),
            Some(_) => return Err("terminated by signal".to_string()),
            None => {}
        }

        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();

            return Err(format!("timed out after {seconds}s"));
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
