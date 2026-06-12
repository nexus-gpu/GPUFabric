use crate::util::security_metrics;
use anyhow::{anyhow, Result};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const TRUSTED_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin:/opt/homebrew/bin";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_OUTPUT_LIMIT: usize = 256 * 1024;

fn validate_program(program: &str) -> Result<()> {
    if program.contains('/') || program.contains('\\') || program.is_empty() {
        security_metrics::record_external_command_rejection();
        return Err(anyhow!("unsafe command path: {}", program));
    }
    Ok(())
}

fn sanitize_args(args: &[&str]) -> Result<()> {
    if args.iter().any(|arg| arg.as_bytes().contains(&0)) {
        security_metrics::record_external_command_rejection();
        return Err(anyhow!("command argument contains NUL byte"));
    }
    Ok(())
}

fn cap_vec(mut bytes: Vec<u8>, limit: usize) -> Vec<u8> {
    if bytes.len() > limit {
        bytes.truncate(limit);
    }
    bytes
}

pub fn run_command_limited(
    program: &str,
    args: &[&str],
    timeout: Duration,
    output_limit: usize,
) -> Result<Output> {
    validate_program(program)?;
    sanitize_args(args)?;

    let mut child = Command::new(program)
        .args(args)
        .env("PATH", TRUSTED_PATH)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("failed to start {}: {}", program, e))?;

    let start = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|e| anyhow!("failed to poll {}: {}", program, e))?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|e| anyhow!("failed to collect {} output: {}", program, e))?;
            return Ok(Output {
                status: output.status,
                stdout: cap_vec(output.stdout, output_limit),
                stderr: cap_vec(output.stderr, output_limit),
            });
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            security_metrics::record_external_command_rejection();
            return Err(anyhow!("{} timed out after {:?}", program, timeout));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub fn run_command_default(program: &str, args: &[&str]) -> Result<Output> {
    run_command_limited(program, args, DEFAULT_TIMEOUT, DEFAULT_OUTPUT_LIMIT)
}

pub fn run_sudo_noninteractive(
    program: &str,
    args: &[&str],
    timeout: Duration,
    output_limit: usize,
) -> Result<Output> {
    validate_program(program)?;
    sanitize_args(args)?;
    let mut sudo_args = vec!["-n", program];
    sudo_args.extend_from_slice(args);
    run_command_limited("sudo", &sudo_args, timeout, output_limit)
}

#[cfg(not(target_os = "android"))]
pub async fn run_command_limited_async(
    program: &str,
    args: &[&str],
    timeout_duration: Duration,
    output_limit: usize,
) -> Result<Output> {
    use tokio::process::Command as TokioCommand;
    use tokio::time::timeout;

    validate_program(program)?;
    sanitize_args(args)?;

    let child = TokioCommand::new(program)
        .args(args)
        .env("PATH", TRUSTED_PATH)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("failed to start {}: {}", program, e))?;

    let output = timeout(timeout_duration, child.wait_with_output())
        .await
        .map_err(|_| anyhow!("{} timed out after {:?}", program, timeout_duration))?
        .map_err(|e| anyhow!("failed to collect {} output: {}", program, e))?;

    Ok(Output {
        status: output.status,
        stdout: cap_vec(output.stdout, output_limit),
        stderr: cap_vec(output.stderr, output_limit),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_in_program() {
        assert!(run_command_default("/bin/echo", &["x"]).is_err());
    }

    #[test]
    fn caps_output() {
        let out = run_command_limited("printf", &["abcdef"], DEFAULT_TIMEOUT, 3).unwrap();
        assert_eq!(out.stdout, b"abc");
    }
}
