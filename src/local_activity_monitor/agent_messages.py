"""Project recorded agent communication metadata without retaining message bodies."""
from __future__ import annotations

import re

from .operation_records import invocations, redact

ACTIONS = ("send_message", "spawn_agent", "followup_task")
TOOLS = {prefix+action: action for prefix in ("", "collaboration.", "functions.", "functions.collaboration.", "collaboration__") for action in ACTIONS}
IDENTIFIER = re.compile(r"[A-Za-z0-9_./:@-]{1,256}\Z")
HEADER = re.compile(r"\AMessage Type: (MESSAGE|NEW_TASK|FINAL_ANSWER)\r?\nTask name: ([A-Za-z0-9_./:@-]{1,256})\r?\nSender: ([A-Za-z0-9_./:@-]{1,256})\r?\nPayload:\r?\n")


def identity(value):
    return value if isinstance(value, str) and IDENTIFIER.fullmatch(value) else None


def agent_messages(payload, context=None, calls=None):
    """Return one descriptor per recognized invocation, with on-demand detail indexes."""
    if not isinstance(payload, dict):
        return []
    context = context if isinstance(context, dict) else {}
    sender = identity(context.get("agent_name")) or identity(context.get("agent_id"))
    result = []
    for invocation_index, (tool, arguments, nested) in enumerate(invocations(payload) if calls is None else calls):
        action = TOOLS.get(tool)
        if action is None:
            continue
        arguments = arguments if isinstance(arguments, dict) else {}
        target = identity(arguments.get("target")) or identity(arguments.get("recipient"))
        task = identity(arguments.get("task_name")) if action == "spawn_agent" else None
        result.append({"action": action, "tool": tool, "target": target, "task_name": task,
                       "sender": sender, "direction": "outgoing", "nested": bool(nested),
                       "index": len(result), "invocation_index": invocation_index,
                       "recognition": "literal_call_site" if nested else "direct_tool_call"})
    return result


def incoming_message(text):
    """Recognize an exact recorded envelope, not its payload or a verified delivery."""
    if not isinstance(text, str):
        return None
    match = HEADER.match(text)
    if match is None:
        return None
    message_type, target, sender = match.groups()
    return {"action": "receive_message", "message_type": message_type,
            "target": target, "sender": sender, "task_name": target,
            "direction": "incoming", "nested": False, "index": 0,
            "invocation_index": None, "recognition": "recorded_message_header"}


def parse_message_detail(payload, index=0, mask=True):
    """Select recorded communication arguments on demand, never infer inherited context."""
    if not isinstance(payload, dict) or type(index) is not int or index < 0 or type(mask) is not bool:
        return None
    calls = invocations(payload)
    descriptors = agent_messages(payload, calls=calls)
    if index >= len(descriptors):
        return None
    descriptor = descriptors[index]
    arguments = calls[descriptor["invocation_index"]][1]
    if not isinstance(arguments, dict):
        return {"metadata": descriptor, "request": None, "context_state": "not_recorded", "masked": mask}
    request = {key: arguments[key] for key in ("message", "prompt", "context", "fork_turns", "target", "recipient", "task_name", "model", "reasoning_effort") if key in arguments}
    return {"metadata": descriptor, "request": redact(request, limit=None) if mask else request,
            "context_state": "recorded" if "context" in arguments else "not_recorded", "masked": mask}
