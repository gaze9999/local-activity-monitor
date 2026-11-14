//! Exact tool summaries over already selected call winners, streamed at encoding.
use super::{stamp, Call, Projection, Time};
use serde::{Serialize, Serializer};
use serde::ser::SerializeSeq;
use std::cmp::Ordering;
use std::collections::{btree_map, BTreeMap, BinaryHeap};

pub(in crate::sessions) struct ToolStatistics<'a> {
    threads: &'a BTreeMap<&'a str, (Projection, bool)>,
    boundary: Option<Time>,
    enabled: bool,
}

impl<'a> ToolStatistics<'a> {
    pub(in crate::sessions) fn new(threads: &'a BTreeMap<&'a str, (Projection, bool)>, boundary: Option<Time>, enabled: bool) -> Self {
        Self { threads, boundary, enabled }
    }

    fn eligible(&self, call: &Call) -> bool {
        self.boundary.is_none_or(|cutoff| call.time.is_some_and(|time| time >= cutoff))
    }
}

#[derive(Serialize)]
struct Row<'a> {
    tool: &'a str,
    nested: bool,
    calls: usize,
    thread_count: usize,
    returned: Option<usize>,
    known_duration_count: Option<usize>,
    average_ms: Option<f64>,
    p99_ms: Option<f64>,
    last_at: Option<String>,
    last_response_at: Option<String>,
}

struct Cursor<'a> {
    tool: &'a str,
    count: usize,
    thread: usize,
    call: &'a Call,
    rest: btree_map::Iter<'a, String, usize>,
}
impl PartialEq for Cursor<'_> { fn eq(&self, other: &Self) -> bool { self.tool == other.tool && self.thread == other.thread } }
impl Eq for Cursor<'_> {}
impl Ord for Cursor<'_> {
    fn cmp(&self, other: &Self) -> Ordering { other.tool.cmp(self.tool).then(other.thread.cmp(&self.thread)) }
}
impl PartialOrd for Cursor<'_> { fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) } }

impl Serialize for ToolStatistics<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        if !self.enabled { return sequence.end(); }
        let (mut call_count, mut nested_count) = (0, 0);
        for (projection, _) in self.threads.values() {
            for call in projection.calls.values().filter(|call| self.eligible(call)) {
                call_count += 1;
                nested_count += usize::from(call.nested.as_ref().is_some_and(|nested| !nested.tools.is_empty()));
            }
        }
        // These capacities depend on selected calls, never on nested invocation count.
        let mut primary = Vec::with_capacity(call_count);
        let mut nested = BinaryHeap::with_capacity(nested_count);
        let mut durations = Vec::with_capacity(call_count);
        for (thread, (projection, _)) in self.threads.values().enumerate() {
            for call in projection.calls.values().filter(|call| self.eligible(call)) {
                primary.push((thread, call));
                if let Some(tools) = call.nested.as_ref().map(|nested| &nested.tools) {
                    let mut rest = tools.iter();
                    if let Some((tool, &count)) = rest.next() { nested.push(Cursor { tool, count, thread, call, rest }); }
                }
            }
        }
        primary.sort_unstable_by(|(left_thread, left), (right_thread, right)| left.tool.cmp(&right.tool).then(left_thread.cmp(right_thread)));
        let mut next = 0;
        while next < primary.len() || !nested.is_empty() {
            if next < primary.len() && nested.peek().is_none_or(|cursor| primary[next].1.tool.as_str() <= cursor.tool) {
                let tool = primary[next].1.tool.as_str();
                let (mut calls, mut thread_count, mut returned) = (0, 0, 0);
                let (mut last_at, mut last_response_at, mut previous_thread) = (None, None, None);
                durations.clear();
                while next < primary.len() && primary[next].1.tool == tool {
                    let (thread, call) = primary[next];
                    calls += 1;
                    if previous_thread != Some(thread) { thread_count += 1; previous_thread = Some(thread); }
                    last_at = last_at.max(call.completed.or(call.time));
                    if let Some(completed) = call.completed { returned += 1; last_response_at = last_response_at.max(Some(completed)); }
                    if let Some(duration) = call.duration_ms() { durations.push(duration); }
                    next += 1;
                }
                let (average_ms, p99_ms) = metrics(&mut durations);
                sequence.serialize_element(&Row { tool, nested: false, calls, thread_count, returned: Some(returned), known_duration_count: Some(durations.len()), average_ms, p99_ms, last_at: last_at.map(stamp), last_response_at: last_response_at.map(stamp) })?;
            } else {
                let tool = nested.peek().expect("nonempty remaining nested group").tool;
                let (mut calls, mut thread_count, mut last_at, mut previous_thread) = (0, 0, None, None);
                while nested.peek().is_some_and(|cursor| cursor.tool == tool) {
                    let mut cursor = nested.pop().expect("peeked nested cursor");
                    calls += cursor.count;
                    if previous_thread != Some(cursor.thread) { thread_count += 1; previous_thread = Some(cursor.thread); }
                    last_at = last_at.max(cursor.call.completed.or(cursor.call.time));
                    if let Some((next_tool, &count)) = cursor.rest.next() { cursor.tool = next_tool; cursor.count = count; nested.push(cursor); }
                }
                sequence.serialize_element(&Row { tool, nested: true, calls, thread_count, returned: None, known_duration_count: None, average_ms: None, p99_ms: None, last_at: last_at.map(stamp), last_response_at: None })?;
            }
        }
        sequence.end()
    }
}

fn metrics(durations: &mut [i64]) -> (Option<f64>, Option<f64>) {
    if durations.is_empty() { return (None, None); }
    let sum: u128 = durations.iter().map(|&duration| duration as u128).sum();
    let average = round_two(ratio(sum, durations.len() as u64));
    durations.sort_unstable();
    let position = (durations.len() - 1) as f64 * 0.99;
    let lower = position as usize;
    let upper = (lower + 1).min(durations.len() - 1);
    let percentile = durations[lower] as f64 + (durations[upper] - durations[lower]) as f64 * (position - lower as f64);
    (Some(average), Some(round_two(percentile)))
}

fn round_two(value: f64) -> f64 { format!("{value:.2}").parse().expect("finite formatted statistic") }

/// Correctly rounded integer division, avoiding a rounded float sum before division.
fn ratio(numerator: u128, denominator: u64) -> f64 {
    if numerator == 0 { return 0.0; }
    let denominator = u128::from(denominator);
    let mut exponent = (128 - numerator.leading_zeros()) as i32 - (128 - denominator.leading_zeros()) as i32;
    if if exponent >= 0 { numerator < (denominator << exponent) } else { (numerator << -exponent) < denominator } { exponent -= 1; }
    let shift = 52 - exponent;
    let (scaled, denominator) = if shift >= 0 { (numerator << shift, denominator) } else { (numerator, denominator << -shift) };
    let quotient = scaled / denominator;
    let remainder = scaled % denominator;
    let quotient = quotient + u128::from(remainder > denominator - remainder || (remainder == denominator - remainder && quotient & 1 == 1));
    (quotient as f64) * 2f64.powi(exponent - 52)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ActivitySeries, NestedActivity};
    use serde_json::{json, Value};

    fn call(tool: &str, time: Option<&str>, completed: Option<&str>, nested: &[(&str,usize)]) -> Call {
        let mut call: Call = serde_json::from_value(json!({"tool":tool,"time":time,"completed":completed,"sequence":1})).unwrap();
        if !nested.is_empty() { call.nested = Some(NestedActivity { tools:nested.iter().map(|(tool,count)|(tool.to_string(),*count)).collect(),events:vec![],isolated:false,truncated:false,tool_counts_incomplete:false,single_invocation:false }); }
        call
    }
    fn render(threads: &BTreeMap<&str,(Projection,bool)>, boundary: Option<Time>, enabled: bool) -> Value {
        serde_json::to_value(ToolStatistics::new(threads,boundary,enabled)).unwrap()
    }

    #[test]
    fn tool_statistics_source_metrics_and_integer_division() {
        for (sum,count,expected) in [(2450645329790825740u128,26295usize,93198149069816.53),(4543100088254092596,40160,113125002197562.06),(149520640244255177,3303,45268132075160.52)] {
            let mut durations=vec![(sum/count as u128)as i64;count];durations[0]+=(sum%count as u128)as i64;
            assert_eq!(metrics(&mut durations).0,Some(expected));
        }
        assert_eq!(metrics(&mut []),(None,None));
        assert_eq!(metrics(&mut [0]),(Some(0.0),Some(0.0)));
        assert_eq!(metrics(&mut [0,1000]),(Some(500.0),Some(990.0)));
        assert_eq!(metrics(&mut (0..100).collect::<Vec<_>>()).1,Some(98.01));
        assert_eq!(metrics(&mut (0..101).collect::<Vec<_>>()).1,Some(99.0));
        assert_eq!(round_two(ratio(125,1000)),0.12);
        assert_eq!(round_two(ratio(375,1000)),0.38);
    }

    #[test]
    fn tool_statistics_all_calls_missing_zero_and_nested_thread_groups() {
        let start="2026-11-13T01:00:00.000Z";let end="2026-11-13T01:00:01.000Z";
        let mut first=Projection::for_id(Some("first".into()));
        for n in 0..130 { first.calls.insert(format!("call-{n}"),call("a",Some(start),Some(end),&[("a",2),("nested",0)])); }
        first.calls.insert("zero".into(),call("zero",Some(start),Some(start),&[]));
        first.calls.insert("negative".into(),call("zero",Some(end),Some(start),&[]));
        first.calls.insert("unknown".into(),call("unknown",None,Some(end),&[]));
        let mut second=Projection::for_id(Some("second".into()));
        second.calls.insert("pending".into(),call("a",Some(end),None,&[("a",3)]));
        let threads=BTreeMap::from([("first",(first,false)),("second",(second,false))]);
        let rows=render(&threads,None,true);
        assert_eq!(rows[0],json!({"tool":"a","nested":false,"calls":131,"thread_count":2,"returned":130,"known_duration_count":130,"average_ms":1000.0,"p99_ms":1000.0,"last_at":end,"last_response_at":end}));
        assert_eq!(rows[1],json!({"tool":"a","nested":true,"calls":263,"thread_count":2,"returned":null,"known_duration_count":null,"average_ms":null,"p99_ms":null,"last_at":end,"last_response_at":null}));
        assert_eq!(rows[2]["tool"],"nested");assert_eq!(rows[2]["calls"],0);assert_eq!(rows[2]["thread_count"],1);
        assert_eq!(rows[3]["tool"],"unknown");assert_eq!(rows[3]["returned"],1);assert_eq!(rows[3]["known_duration_count"],0);assert!(rows[3]["p99_ms"].is_null());
        assert_eq!(rows[4]["tool"],"zero");assert_eq!(rows[4]["known_duration_count"],2);assert_eq!(rows[4]["average_ms"],0.0);
        assert_eq!(render(&threads,None,false),json!([]));
        let cutoff=super::super::timestamp(Some(end));let scoped=render(&threads,cutoff,true);
        assert_eq!(scoped.as_array().unwrap().iter().map(|row|row["tool"].as_str().unwrap()).collect::<Vec<_>>(),vec!["a","a","zero"]);
        assert_eq!(scoped[0]["calls"],1);assert_eq!(scoped[0]["returned"],0);
    }

    #[test]
    fn tool_statistics_selected_winners_call_payloads_and_serialization() {
        let mut first=Projection::for_id(Some("same".into()));let mut second=Projection::for_id(first.id.clone());
        first.calls.insert("same-call".into(),call("old",Some("2026-11-13T01:00:00Z"),None,&[("stale",9)]));
        second.calls.insert("same-call".into(),call("new",Some("2026-11-13T01:00:01Z"),Some("2026-11-13T01:00:01Z"),&[("new",2)]));
        second.calls.insert("excluded".into(),call("excluded",None,None,&[]));
        let selected=["same-call".to_string()].into();let mut merged=Projection::for_id(first.id.clone());
        merged.merge_snapshot(&first,None,&[None;4]);merged.merge_snapshot(&second,Some(&selected),&[None;4]);
        let mut restored:Projection=serde_json::from_value(serde_json::to_value(&merged).unwrap()).unwrap();
        restored.calls=serde_json::from_value(serde_json::to_value(&merged.calls).unwrap()).unwrap();
        let threads=BTreeMap::from([("same",(merged,true))]);let restored=BTreeMap::from([("same",(restored,true))]);
        assert_eq!(render(&threads,None,true),render(&restored,None,true));
        let statistics=ToolStatistics::new(&threads,None,true);let root=json!({"keep":17,"codex":{"health":"partly_unavailable","read_state":{"overflow_snapshot_incomplete":true},"tool_statistics":"old","other":"preserved"}});
        let text=ActivitySeries::new().snapshot_json(&root,Some(&statistics));let value:Value=serde_json::from_str(&text).unwrap();
        assert_eq!(text.matches("\"tool_statistics\":").count(),1);
        assert_eq!(value["keep"],17);assert_eq!(value["codex"]["other"],"preserved");assert_eq!(value["codex"]["read_state"],root["codex"]["read_state"]);
        assert_eq!(value["codex"]["tool_statistics"],render(&threads,None,true));
        assert_eq!(value["codex"]["tool_statistics"][0]["tool"],"new");assert_eq!(value["codex"]["tool_statistics"][1]["nested"],true);
        assert_eq!(ActivitySeries::new().snapshot_json(&Value::Null,Some(&statistics)),"null");
    }
}
