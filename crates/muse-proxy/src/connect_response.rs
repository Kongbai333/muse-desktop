//! One bounded, standards-based CONNECT response parser for both proxy hops.
use super::*;

pub(super) async fn read(
    socket: &mut UpstreamStream,
    context: TraceContext,
    outer: bool,
    elapsed: &TraceSpan,
) -> Result<Vec<u8>, ProxyError> {
    let read_stage = if outer {
        TraceStage::SystemConnectRead
    } else {
        TraceStage::HttpConnectRead
    };
    let status_stage = if outer {
        TraceStage::SystemConnect
    } else {
        TraceStage::HttpConnect
    };
    let parse_stage = if outer {
        TraceStage::SystemConnectParse
    } else {
        TraceStage::HttpConnectParse
    };
    let protocol = if outer {
        ProxyError::SystemProtocol
    } else {
        ProxyError::Protocol
    };
    let span = TraceSpan::new(context, read_stage);
    TraceEvent::new(context, read_stage, TraceOutcome::Start).emit();
    let mut data = Vec::new();
    let mut cumulative = 0usize;
    let mut informational = 0usize;
    loop {
        let mut headers = [httparse::EMPTY_HEADER; 128];
        let mut response = httparse::Response::new(&mut headers);
        match response.parse(&data) {
            Ok(httparse::Status::Complete(end)) if cumulative + end <= MAX_HEAD => {
                let code = response.code.unwrap_or(0);
                if !(100..600).contains(&code) || code == 101 {
                    TraceEvent::new(context, parse_stage, TraceOutcome::Protocol).emit();
                    return Err(protocol);
                }
                let mut event = TraceEvent::new(
                    context,
                    status_stage,
                    if (200..300).contains(&code) {
                        TraceOutcome::Ok
                    } else if code < 200 {
                        TraceOutcome::Start
                    } else if code == 407 {
                        TraceOutcome::Authentication
                    } else {
                        TraceOutcome::Protocol
                    },
                );
                event.http_status = Some(code as i32);
                event.duration_ms = Some(elapsed.elapsed_ms());
                event.emit();
                if code < 200 {
                    informational += 1;
                    cumulative += end;
                    if informational > 8 {
                        return Err(protocol);
                    }
                    data.drain(..end);
                    continue;
                }
                span.end(TraceOutcome::Ok, None);
                if (200..300).contains(&code) {
                    return Ok(data[end..].to_vec());
                }
                return Err(if outer {
                    ProxyError::SystemRejected
                } else if code == 407 {
                    ProxyError::Authentication
                } else {
                    ProxyError::Connect
                });
            }
            Ok(httparse::Status::Partial) if cumulative + data.len() < MAX_HEAD => {}
            _ => {
                TraceEvent::new(context, parse_stage, TraceOutcome::Protocol).emit();
                return Err(protocol);
            }
        }
        let mut chunk = [0; 4096];
        let count = socket.read(&mut chunk).await.map_err(|error| {
            span.io(&error);
            if outer {
                ProxyError::SystemUnavailable
            } else {
                ProxyError::Connect
            }
        })?;
        if count == 0 {
            span.end(TraceOutcome::ResponseEof, None);
            return Err(protocol);
        }
        data.extend_from_slice(&chunk[..count]);
    }
}
