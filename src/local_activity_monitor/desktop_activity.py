"""Project bounded desktop activity metadata, excluding identities and file paths."""
import re
from .codex_metadata import date, text

HOST = re.compile(r'(?:local|remote-control:[A-Za-z0-9_-]{1,160})\Z')
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
            if not isinstance(host,str) or not HOST.fullmatch(host) or not isinstance(identity,str) or not THREAD.fullmatch(identity):
                continue
            when = row.get('updatedAtMs',row.get('timestampMs'))
            timestamp = date(when/1000) if type(when) in (int,float) else date(row.get('updatedAt'))
            activity.append({'thread_id':identity,'host_id':host,'timestamp':timestamp,'activity_type':'dot'})
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
            activity.append({'thread_id':identity,'host_id':entry.get('_host_id'),'timestamp':entry.get('updated_at'),'activity_type':'dot'})
    for host, machine in hosts.items():
        machine['thread_count'] = sum(entry.get('_host_id') == host for entry in entries.values())
        machine['connection_status'] = 'unknown'
    dots.update(computers=sorted(hosts.values(),key=lambda item:item['last_activity_at'] or '',reverse=True)[:100],
                activity=sorted(activity,key=lambda item:item['timestamp'] or '',reverse=True)[:500])
