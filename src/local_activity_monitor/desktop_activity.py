"""Project bounded desktop activity metadata, excluding identities and file paths."""
import re
from .codex_metadata import date, text

HOST = re.compile(r'(?:local|remote-control:[A-Za-z0-9_-]{1,160})\Z')
CLOUD_HOST = re.compile(r'(?:durable|chatgpt:[A-Za-z0-9_-]{1,160})\Z')
THREAD = re.compile(r'[a-fA-F0-9-]{36}\Z')


def desktop_activity(state, entries, dots):
    atom = state.get('electron-persisted-atom-state', {})
    if not isinstance(atom, dict):
        return
    hosts, activity = {}, []
    for entry in entries.values():
        host = entry.get('_host_id')
        if isinstance(host, str) and HOST.fullmatch(host):
            item = hosts.setdefault(host, {'host_id':host, 'last_activity_at':None, 'thread_count':0})
            item['thread_count'] += 1
            item['last_activity_at'] = max(item['last_activity_at'] or '', entry.get('updated_at') or '') or None
    for key, values in atom.items():
        if not key.startswith('remote-thread-summaries-v3:') or not isinstance(values, list):
            continue
        host = key.removeprefix('remote-thread-summaries-v3:')
        if not HOST.fullmatch(host):
            continue
        for value in values[:1000]:
            if not isinstance(value, dict) or not isinstance(value.get('conversationId'), str) or not THREAD.fullmatch(value['conversationId']):
                continue
            identity = value['conversationId']
            updated = date(value.get('updatedAt'))
            existing = entries.get(identity, {})
            if (existing.get('updated_at') or '') > (updated or ''):
                continue
            trigger = value.get('threadSource')
            trigger = trigger if trigger in ('user','dot','orbit','automation','heartbeat','schedule','subagent') else 'unknown'
            runtime = value.get('threadRuntimeStatus')
            status = runtime.get('type') if isinstance(runtime, dict) else None
            item = {'thread_name':text(value.get('title')), 'created_at':date(value.get('createdAt')), 'updated_at':updated,
                    'environment':'remote','activity_type':'codex','trigger':trigger,'_host_id':host,'project_scope':'unknown'}
            if status in ('running','completed','idle','error'):
                item['status'] = status
            parent = value.get('parentThreadId')
            if isinstance(parent,str) and THREAD.fullmatch(parent):
                item['execution'] = {'parent_thread_id':parent}
            entries[identity] = existing | item
            machine = hosts.setdefault(host, {'host_id':host,'last_activity_at':None,'thread_count':0})
            machine['last_activity_at'] = max(machine['last_activity_at'] or '',updated or '') or None
    snapshots = atom.get('orbit-activity-snapshots-v1')
    for snapshot in snapshots[:200] if isinstance(snapshots,list) else ():
        rows = snapshot.get('data') if isinstance(snapshot,dict) else None
        for row in rows[:1000] if isinstance(rows,list) else ():
            if not isinstance(row,dict):
                continue
            host = row.get('hostId')
            identity = row.get('threadId',row.get('conversationId'))
            if not isinstance(host,str) or not (HOST.fullmatch(host) or CLOUD_HOST.fullmatch(host)) or not isinstance(identity,str) or not THREAD.fullmatch(identity):
                continue
            when = row.get('updatedAtMs',row.get('timestampMs'))
            timestamp = date(when/1000) if type(when) in (int,float) else date(row.get('updatedAt'))
            activity.append({'thread_id':identity,'host_id':host,'timestamp':timestamp,'activity_type':'dot',
                             'environment':'cloud' if CLOUD_HOST.fullmatch(host) else 'local' if host=='local' else 'remote'})
            if HOST.fullmatch(host):
                machine = hosts.setdefault(host, {'host_id':host,'last_activity_at':None,'thread_count':0})
                machine['last_activity_at'] = max(machine['last_activity_at'] or '',timestamp or '') or None
    for event in dots.get('events') or ():
        entry = entries.get(event['thread_id'], {})
        host = event.get('host_id') or entry.get('_host_id')
        if isinstance(host,str) and HOST.fullmatch(host):
            activity.append({'thread_id':event['thread_id'],'host_id':host,'timestamp':event['timestamp'],'activity_type':'output'})
            machine = hosts.setdefault(host, {'host_id':host,'last_activity_at':None,'thread_count':0})
            machine['last_activity_at'] = max(machine['last_activity_at'] or '',event['timestamp'] or '') or None
    for identity, entry in entries.items():
        if entry.get('trigger') in ('dot','orbit'):
            activity.append({'thread_id':identity,'host_id':entry.get('_host_id'),'timestamp':entry.get('updated_at'),'activity_type':'dot',
                             'environment':entry.get('environment','unknown')})
    for host, machine in hosts.items():
        machine['thread_count'] = sum(entry.get('_host_id') == host for entry in entries.values())
        machine['connection_status'] = 'unknown'
    dots.update(computers=sorted(hosts.values(),key=lambda item:item['last_activity_at'] or '',reverse=True)[:100],
                activity=sorted(activity,key=lambda item:item['timestamp'] or '',reverse=True)[:500])


def project_dots(state, entries, dots):
    atom = state.get("electron-persisted-atom-state", {})
    outputs = atom.get("orbit-outputs-v1") if isinstance(atom, dict) else None
    if dots is not None:
        snapshots = atom.get("orbit-activity-snapshots-v1") if isinstance(atom, dict) else None
        dots.update(events=[] if isinstance(outputs, list) else None, activity_items=sum(len(item["data"]) for item in snapshots[:2000] if isinstance(item, dict) and isinstance(item.get("data"), list)) if isinstance(snapshots, list) else None)
    if isinstance(outputs, list):
        seen = set()
        for output in outputs[:2000]:
            if dots is not None and isinstance(output, dict) and isinstance(output.get("threadId"), str) and re.fullmatch(r"[a-fA-F0-9-]{36}", output["threadId"]):
                artifact = output.get("artifact")
                if isinstance(artifact, dict):
                    produced = artifact.get("producedAtMs")
                    when = date(produced/1000) if type(produced) in (int, float) else None
                    kind = artifact.get("type")
                    kind = kind if isinstance(kind, str) and re.fullmatch(r"[a-zA-Z0-9_.:-]{1,80}", kind) else None
                    identity = (output["threadId"], output.get("turnId") if isinstance(output.get("turnId"), str) else None, when, kind)
                    if identity not in seen:
                        seen.add(identity)
                        host = output.get("hostId")
                        host = host if isinstance(host,str) and re.fullmatch(r"(?:local|remote-control:[A-Za-z0-9_-]{1,160})",host) else None
                        dots["events"].append({"thread_id": output["threadId"], "timestamp": when, "artifact_type": kind, "host_id":host})
            if isinstance(output, dict) and isinstance(output.get("threadId"), str) and output["threadId"] in entries:
                entries[output["threadId"]]["trigger"] = "dot"
        if dots is not None:
            dots["_retained_events"] = dots["events"]
            dots["events"] = sorted(dots["events"], key=lambda item:item["timestamp"] or "", reverse=True)[:500]
    if dots is not None:
        desktop_activity(state, entries, dots)


def read_dots(home, window='all'):
    from datetime import datetime, timezone
    import json
    import stat
    from .codex_metadata import APP_STATE_LIMIT
    from .activity_windows import cutoff, contains
    boundary = cutoff(window)
    checked = datetime.now(timezone.utc).isoformat(timespec='seconds')
    result = {'health':'error', 'checked_at':checked, 'source':'desktop_cache', 'window':window}
    path = home/'.codex-global-state.json'
    try:
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode) or path.is_symlink() or getattr(info, 'st_file_attributes', 0)&0x400 or info.st_size>APP_STATE_LIMIT:
            return result
        with path.open('rb') as stream:
            raw = stream.read(APP_STATE_LIMIT+1)
        if len(raw)>APP_STATE_LIMIT:
            return result
        value = json.loads(raw)
        if not isinstance(value,dict):
            return result
        dots = {}
        project_dots(value, {}, dots)
        events = dots.pop('_retained_events', dots.get('events'))
        if events is not None:
            events = [event for event in events if contains(event, boundary)]
            dots.update(events=sorted(events, key=lambda item:item.get('timestamp') or '', reverse=True)[:500], total=len(events))
        dots['activity'] = [event for event in dots.get('activity', []) if contains(event, boundary)]
        return result|dots|{'health':'ok'}
    except (OSError, ValueError, RecursionError):
        return result
