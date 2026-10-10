"""Bounded native source notifications. Never read source bodies or poll a clock."""
import ctypes
import os
from pathlib import Path
import re
import select
import struct
import sys
import threading


def source_path(relative):
    parts = Path(str(relative).replace('\\', '/')).parts
    if not parts or '..' in parts:
        return False
    if parts[-1].endswith('-shm'):
        return False
    if parts[0] in ('sessions', 'archived_sessions', 'sqlite', 'logs', 'desktop-logs'):
        return True
    if parts[0] == 'monitoring':
        return len(parts) == 1 or parts[1] == 'jev-monitor.json'
    if parts[0] == 'plugins':
        if len(parts) == 1:
            return True
        if parts[1] != 'cache':
            return False
        if len(parts) <= 5:
            return True
        tail = parts[5:]
        return tail in [('plugin.json',), ('.mcp.json',), ('.codex-plugin',), ('.codex-plugin', 'plugin.json')] or tail[0] == 'skills' and (len(tail) <= 2 or len(tail) == 3 and tail[2] == 'SKILL.md')
    return len(parts) == 1 and (parts[0] in ('config.toml', '.codex-global-state.json', 'session_index.jsonl')
                              or re.fullmatch(r'(?:state|logs)_\d+\.sqlite(?:-wal|-shm)?', parts[0]) is not None)


def git_metadata_path(relative):
    parts = Path(str(relative).replace('\\', '/')).parts
    if not parts:
        return True
    if '..' in parts or parts[-1].endswith('.lock'):
        return False
    if parts[0] == 'refs':
        return True
    if parts[0] == 'worktrees':
        return len(parts) <= 2 or len(parts) == 3 and parts[2] in ('HEAD', 'gitdir', 'commondir', 'locked')
    return len(parts) == 1 and parts[0] in ('HEAD', 'packed-refs', 'gitdir', 'commondir', 'locked')


def managed_worktree_path(relative):
    parts = Path(str(relative).replace('\\', '/')).parts
    return '..' not in parts and (len(parts) <= 2 or len(parts) == 3 and parts[-1] == '.git')


class SourceWatch:
    LIMIT = 5000

    def __init__(self, root, changed, accepts=lambda relative: True):
        self.root, self.changed, self.accepts = Path(root), changed, accepts
        self.stopped = threading.Event()
        self.ready = threading.Event()
        self.health = 'starting'
        self.error_type = None
        self.cancel = None
        self.cancel_lock = threading.Lock()
        self.thread = threading.Thread(target=self.run, name='source-notifications', daemon=True)

    def start(self):
        self.thread.start()
        return self

    def close(self):
        self.stopped.set()
        with self.cancel_lock:
            if self.cancel:
                self.cancel()
        self.thread.join(5)
        if self.thread.is_alive():
            raise RuntimeError('Source notification worker did not stop')

    def run(self):
        try:
            if not self.root.is_dir() or self.root.is_symlink():
                raise OSError('Unavailable source directory')
            if os.name == 'nt':
                self.windows()
            elif sys.platform.startswith('linux'):
                self.linux()
            elif hasattr(select, 'kqueue'):
                self.bsd()
            else:
                raise OSError('Native source notifications unsupported')
        except (OSError, ValueError) as error:
            if not self.stopped.is_set():
                self.health, self.error_type = 'unavailable', type(error).__name__
                self.changed()
        finally:
            self.ready.set()

    def emit(self, names):
        if not self.stopped.is_set() and (names is None or any(self.accepts(name) for name in names)):
            self.changed()

    def directories(self):
        found = [self.root]
        for directory in found:
            try:
                entries = os.scandir(directory)
            except FileNotFoundError:
                if directory == self.root:
                    raise
                continue
            with entries:
                for index, entry in enumerate(entries):
                    if index >= 10000:
                        raise OSError('Source directory entry limit')
                    relative = Path(entry.path).relative_to(self.root)
                    if entry.is_dir(follow_symlinks=False) and self.accepts(relative):
                        if len(found) >= self.LIMIT:
                            raise OSError('Source directory limit')
                        found.append(Path(entry.path))
        return found

    def windows(self):
        from ctypes import wintypes as w
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        class Overlapped(ctypes.Structure):
            _fields_ = [('Internal', ctypes.c_size_t), ('InternalHigh', ctypes.c_size_t),
                        ('Offset', w.DWORD), ('OffsetHigh', w.DWORD), ('hEvent', w.HANDLE)]
        definitions = {
            'CreateFileW': ([w.LPCWSTR, w.DWORD, w.DWORD, w.LPVOID, w.DWORD, w.DWORD, w.HANDLE], w.HANDLE),
            'CreateEventW': ([w.LPVOID, w.BOOL, w.BOOL, w.LPCWSTR], w.HANDLE),
            'ReadDirectoryChangesW': ([w.HANDLE, w.LPVOID, w.DWORD, w.BOOL, w.DWORD, w.LPVOID, ctypes.POINTER(Overlapped), w.LPVOID], w.BOOL),
            'GetOverlappedResult': ([w.HANDLE, ctypes.POINTER(Overlapped), ctypes.POINTER(w.DWORD), w.BOOL], w.BOOL),
            'CancelIoEx': ([w.HANDLE, ctypes.POINTER(Overlapped)], w.BOOL),
            'ResetEvent': ([w.HANDLE], w.BOOL), 'CloseHandle': ([w.HANDLE], w.BOOL),
        }
        for name, (arguments, result) in definitions.items():
            getattr(kernel, name).argtypes, getattr(kernel, name).restype = arguments, result
        handle = kernel.CreateFileW(str(self.root), 1, 7, None, 3, 0x02000000 | 0x40000000, None)
        if handle == w.HANDLE(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        event = kernel.CreateEventW(None, True, False, None)
        if not event:
            kernel.CloseHandle(handle)
            raise ctypes.WinError(ctypes.get_last_error())
        pending = Overlapped(hEvent=event)
        buffer = ctypes.create_string_buffer(65536)
        arm = threading.Lock()
        def cancel():
            with arm:
                kernel.CancelIoEx(handle, ctypes.byref(pending))
        self.cancel = cancel
        try:
            while not self.stopped.is_set():
                with arm:
                    if self.stopped.is_set():
                        break
                    kernel.ResetEvent(event)
                    if not kernel.ReadDirectoryChangesW(handle, buffer, len(buffer), True, 1 | 2 | 8 | 16,
                                                        None, ctypes.byref(pending), None):
                        raise ctypes.WinError(ctypes.get_last_error())
                self.health = 'ok'
                self.ready.set()
                transferred = w.DWORD()
                if not kernel.GetOverlappedResult(handle, ctypes.byref(pending), ctypes.byref(transferred), True):
                    if self.stopped.is_set():
                        break
                    if ctypes.get_last_error() == 1022:
                        self.emit(None)
                        continue
                    raise ctypes.WinError(ctypes.get_last_error())
                raw, names, offset = buffer.raw[:transferred.value], [], 0
                while offset + 12 <= len(raw):
                    following, action, length = struct.unpack_from('<III', raw, offset)
                    if length % 2 or offset + 12 + length > len(raw):
                        raise ValueError('Invalid source notification')
                    name = raw[offset+12:offset+12+length].decode('utf-16-le')
                    # Child journal writes also modify the parent directory on Windows.
                    # Creation/removal/rename still request discovery of new sources.
                    if action != 3 or not (self.root/name).is_dir():
                        names.append(name)
                    if not following:
                        break
                    offset += following
                self.emit(names if raw else None)
        finally:
            with self.cancel_lock:
                self.cancel = None
                kernel.CancelIoEx(handle, ctypes.byref(pending))
                kernel.CloseHandle(handle)
                kernel.CloseHandle(event)

    def linux(self):
        library = ctypes.CDLL(None, use_errno=True)
        library.inotify_init1.argtypes, library.inotify_init1.restype = [ctypes.c_int], ctypes.c_int
        library.inotify_add_watch.argtypes, library.inotify_add_watch.restype = [ctypes.c_int, ctypes.c_char_p, ctypes.c_uint32], ctypes.c_int
        descriptor = library.inotify_init1(os.O_NONBLOCK | os.O_CLOEXEC)
        if descriptor < 0:
            raise OSError(ctypes.get_errno(), 'inotify init failed')
        reader, writer = os.pipe()
        paths, watches = {}, set()
        def add_directories():
            for directory in self.directories():
                if directory in watches:
                    continue
                key = library.inotify_add_watch(descriptor, os.fsencode(directory), 0x00000FCA)
                if key < 0:
                    if ctypes.get_errno() == 2 and directory != self.root:
                        continue
                    raise OSError(ctypes.get_errno(), 'inotify watch failed')
                paths[key] = directory
                watches.add(directory)
        self.cancel = lambda: os.write(writer, b'x')
        try:
            add_directories()
            self.health = 'ok'
            self.ready.set()
            while not self.stopped.is_set():
                available, _, _ = select.select([descriptor, reader], [], [])
                if reader in available:
                    break
                raw, offset, names = os.read(descriptor, 65536), 0, []
                while offset + 16 <= len(raw):
                    key, mask, cookie, length = struct.unpack_from('iIII', raw, offset)
                    name = os.fsdecode(raw[offset+16:offset+16+length].split(b'\0', 1)[0])
                    if mask & 0x00004000:
                        names = None
                    elif key in paths and names is not None:
                        names.append((paths[key] / name).relative_to(self.root))
                    if mask & 0x00008000 and key in paths:
                        watches.discard(paths.pop(key))
                    offset += 16 + length
                self.emit(names)
                add_directories()
        finally:
            with self.cancel_lock:
                self.cancel = None
                for descriptor in (descriptor, reader, writer):
                    os.close(descriptor)

    def bsd(self):
        queue = select.kqueue()
        reader, writer = os.pipe()
        descriptors = {}
        flags = select.KQ_NOTE_WRITE | select.KQ_NOTE_EXTEND | select.KQ_NOTE_DELETE | select.KQ_NOTE_RENAME
        def enroll():
            paths = set(self.directories())
            for directory in tuple(paths):
                try:
                    entries = os.scandir(directory)
                except FileNotFoundError:
                    paths.discard(directory)
                    continue
                with entries:
                    for index, entry in enumerate(entries):
                        if index >= 10000 or len(paths) >= self.LIMIT:
                            raise OSError('Source file limit')
                        if entry.is_file(follow_symlinks=False) and self.accepts(Path(entry.path).relative_to(self.root)):
                            paths.add(Path(entry.path))
            for path in paths-set(descriptors.values()):
                try:
                    descriptor = os.open(path, getattr(os, 'O_EVTONLY', os.O_RDONLY) | os.O_NONBLOCK)
                except FileNotFoundError:
                    paths.discard(path)
                    continue
                descriptors[descriptor] = path
                queue.control([select.kevent(descriptor, filter=select.KQ_FILTER_VNODE,
                              flags=select.KQ_EV_ADD | select.KQ_EV_CLEAR, fflags=flags)], 0)
            signatures = {}
            for path in paths:
                try:
                    stat = path.stat()
                    signatures[path] = (stat.st_mtime_ns, stat.st_size) if path.is_file() else None
                except FileNotFoundError:
                    pass
            return signatures
        self.cancel = lambda: os.write(writer, b'x')
        try:
            queue.control([select.kevent(reader, filter=select.KQ_FILTER_READ, flags=select.KQ_EV_ADD)], 0)
            signatures = enroll()
            self.health = 'ok'
            self.ready.set()
            while not self.stopped.is_set():
                events = queue.control(None, 64)
                if any(event.ident == reader for event in events):
                    break
                for event in events:
                    path = descriptors.get(event.ident)
                    if path is not None:
                        if event.fflags & (select.KQ_NOTE_DELETE | select.KQ_NOTE_RENAME):
                            os.close(event.ident)
                            descriptors.pop(event.ident)
                updated = enroll()
                self.emit([path.relative_to(self.root) for path in signatures.keys() | updated.keys()
                           if signatures.get(path, False) != updated.get(path, False)])
                signatures = updated
        finally:
            with self.cancel_lock:
                self.cancel = None
                queue.close()
                for descriptor in (*descriptors, reader, writer):
                    os.close(descriptor)
