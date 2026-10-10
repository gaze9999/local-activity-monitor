import json
from contextlib import closing
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from local_activity_monitor.server import Dashboard, handler
from local_activity_monitor.worktree_info import WorktreeCollector, local_path, parse_worktrees


THREAD = "00000000-0000-0000-0000-000000000001"


class WorktreeTests(unittest.TestCase):
    def test_git_metadata_notifications_invalidate_recent_cache(self):
        self.collector.refresh(self.projects, [], {})
        roots = self.collector.notification_roots()
        self.assertIn(self.repo/'.git', roots)
        linked_metadata = Path(self.git(self.linked, 'rev-parse', '--absolute-git-dir').decode().strip())
        self.assertIn(linked_metadata, roots)
        app = Dashboard(self.home, codex=False)
        app.worktrees = self.collector
        app.observations['worktrees'] = True
        app.watch_sources()
        try:
            self.collector.next_read = time.monotonic()+30
            app.source_changed.clear()
            self.git(self.repo, 'switch', '-q', '-c', 'fixture-notified')
            self.assertTrue(app.source_changed.wait(3))
            self.assertEqual(self.collector.next_read, 0)
            value = self.collector.refresh(self.projects, [], {})
            self.assertTrue(any(item['branch'] == 'fixture-notified' for item in value['items']))
        finally:
            app.close_sources()

    def setUp(self):
        if shutil.which("git") is None:
            self.skipTest("Git is unavailable")
        self.directory = tempfile.TemporaryDirectory(prefix="lam-worktree-test-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        self.home = self.root/"codex"
        self.home.mkdir()
        self.repo = self.root/"private repository"
        self.repo.mkdir()
        self.git(self.repo, "init", "-q", "--initial-branch=main")
        self.git(self.repo, "commit", "-q", "--allow-empty", "-m", "Fixture")
        self.linked = self.home/"worktrees/abcd/linked repository"
        self.linked.parent.mkdir(parents=True)
        self.git(self.repo, "worktree", "add", "-q", "--detach", str(self.linked))
        self.collector = WorktreeCollector(self.home)
        self.projects = {"p1": {"name": "Fixture", "folders": [str(self.repo)]}}

    def git(self, root, *arguments):
        environment = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        return subprocess.run([shutil.which("git"), "-C", str(root), "-c", "core.hooksPath="+os.devnull,
                               "-c", "commit.gpgsign=false", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", *arguments],
                              capture_output=True, env=environment, check=True).stdout

    def test_registered_and_codex_managed_worktrees_with_locks_and_private_details(self):
        self.git(self.repo, "worktree", "lock", "--reason", "PRIVATE_REASON", str(self.linked))
        with patch.object(Path, "read_text", side_effect=AssertionError("workspace content accessed")):
            value = self.collector.refresh(self.projects, [{"thread_id": THREAD}], {THREAD: str(self.linked/"src")})
        self.assertEqual(value["health"], "ok")
        self.assertEqual(value["total"], 2)
        self.assertEqual(value["queries"], 1)
        main = next(item for item in value["items"] if item["branch"] == "main")
        linked = next(item for item in value["items"] if item["managed"])
        self.assertFalse(main["locked"])
        self.assertEqual(main["thread_count"], 0)
        self.assertTrue(linked["detached"])
        self.assertTrue(linked["locked"])
        self.assertEqual(linked["thread_ids"], [THREAD])
        self.assertEqual(linked["project_ids"], ["p1"])
        self.assertNotIn(str(self.repo), json.dumps(value))
        self.assertNotIn("PRIVATE_REASON", json.dumps(value))
        self.assertEqual(self.collector.detail(linked["id"])["path"], str(self.linked))
        self.assertIsNone(self.collector.detail("unknown"))

    def test_cache_deduplicates_repositories_and_updates_associations_without_git_reads(self):
        self.projects["p2"] = {"folders": [str(self.linked)]}
        with patch.object(self.collector, "git_list", wraps=self.collector.git_list) as read:
            first = self.collector.refresh(self.projects, [{"thread_id": THREAD}], {THREAD: str(self.linked)})
            second = self.collector.refresh(self.projects, [{"thread_id": THREAD}], {THREAD: str(self.repo)})
            self.assertEqual(read.call_count, 1)
        self.assertTrue(all(item["project_ids"] == ["p1", "p2"] for item in second["items"]))
        self.assertEqual(first["checked_at"], second["checked_at"])
        self.assertEqual(next(item for item in second["items"] if item["branch"] == "main")["thread_count"], 1)
        self.assertEqual(next(item for item in second["items"] if item["managed"])["thread_count"], 0)

    def test_missing_linked_directory_remains_visible_as_prunable(self):
        self.assertTrue(self.linked.resolve().is_relative_to(self.root))
        shutil.rmtree(self.linked)
        value = self.collector.refresh(self.projects, [], {})
        linked = next(item for item in value["items"] if item["managed"])
        self.assertFalse(linked["available"])
        self.assertTrue(linked["prunable"])

    def test_rejected_paths_and_git_metadata_links_never_launch_git(self):
        self.assertIsNone(local_path("../outside"))
        self.assertIsNone(local_path("//server/share"))
        self.assertIsNone(local_path(str(self.repo)+"\n"))
        original = Path.is_symlink
        self.collector.managed_root = self.home/"absent"
        with patch.object(Path, "is_symlink", lambda path: path == self.repo/".git" or original(path)), patch.object(self.collector, "git_list", side_effect=AssertionError("linked metadata accessed")):
            self.assertEqual(self.collector.refresh(self.projects, [], {})["health"], "partly_unavailable")

    def test_missing_git_failure_limit_and_disabled_states_are_distinct(self):
        with patch("local_activity_monitor.worktree_info.shutil.which", return_value=None):
            self.assertEqual(self.collector.refresh(self.projects, [], {})["health"], "missing_git")
        self.collector.next_read = 0
        with patch.object(self.collector, "git_list", return_value=(None, "timeout")):
            value = self.collector.refresh(self.projects, [], {})
            self.assertEqual(value["health"], "partly_unavailable")
            self.assertEqual(value["failed_queries"], 2)
            self.assertIsNone(value["total"])
            self.assertEqual(value["observed_total"], 0)
            self.assertFalse(value["count_complete"])
            self.assertTrue(all(item["reason"] == "timeout" for item in value["query_failures"]))
        with patch.object(self.collector, "read", side_effect=AssertionError("disabled read")):
            self.assertEqual(self.collector.refresh(self.projects, [], {}, False)["health"], "disabled")
        self.assertIsNone(self.collector.detail("unknown"))
        self.collector.ROOT_LIMIT = 1
        value = self.collector.refresh(self.projects, [], {})
        self.assertTrue(value["limited"])

    def test_nested_worktree_uses_the_most_specific_working_directory(self):
        nested = self.repo/"nested"
        self.git(self.repo, "worktree", "add", "-q", "--detach", str(nested))
        value = self.collector.refresh(self.projects, [{"thread_id": THREAD}], {THREAD: str(nested/"src")})
        self.assertEqual(sum(item["thread_count"] for item in value["items"]), 1)
        self.assertEqual(next(item for item in value["items"] if item["name"] == "nested")["thread_ids"], [THREAD])

    def test_budget_continues_with_remaining_roots_and_retains_previous_results(self):
        other = self.root/"other repository"
        (other/".git").mkdir(parents=True)
        self.projects["p2"] = {"folders": [str(other)]}
        self.collector.TIME_BUDGET = 1
        clock = [0]
        def read(root, timeout):
            clock[0] += 1
            return [{"path": str(root), "commit": "a"*40, "branch": "main"}], "ok"
        with patch("local_activity_monitor.worktree_info.time.monotonic", side_effect=lambda: clock[0]), patch.object(self.collector, "git_list", side_effect=read) as query:
            first = self.collector.refresh(self.projects, [], {})
            self.assertTrue(first["limited"])
            self.assertIsNone(first["total"])
            self.assertEqual(first["observed_total"], 1)
            self.collector.next_read = 0
            second = self.collector.refresh(self.projects, [], {})
            self.assertEqual(query.call_args.args[0], other)
            self.assertEqual(second["observed_total"], 2)
            self.assertTrue(any(item["name"] == self.repo.name for item in second["items"]))

    def test_failed_refresh_preserves_observed_items_with_partial_count(self):
        initial = self.collector.refresh(self.projects, [], {})
        self.assertTrue(initial['count_complete'])
        self.collector.next_read = 0
        with patch.object(self.collector, 'git_list', return_value=(None, 'timeout')):
            value = self.collector.refresh(self.projects, [], {})
        self.assertIsNone(value['total'])
        self.assertEqual(value['observed_total'], 2)
        self.assertEqual(len(value['items']), 2)
        self.assertFalse(value['count_complete'])
        self.assertNotIn(str(self.repo), json.dumps(value['query_failures']))

    def test_git_exit_projects_only_bounded_reason_and_code(self):
        class FailedProcess:
            returncode = 128
            def wait(self, timeout=None):
                return self.returncode
        def launch(*args, **kwargs):
            kwargs['stderr'].write(b"fatal: detected dubious ownership in repository PRIVATE_PATH TOKEN_SECRET")
            return FailedProcess()
        with patch('local_activity_monitor.worktree_info.subprocess.Popen', side_effect=launch):
            value = self.collector.refresh(self.projects, [], {})
        self.assertIsNone(value['total'])
        self.assertEqual(value['query_failures'][0]['reason'], 'ownership_rejected')
        self.assertEqual(value['query_failures'][0]['exit_code'], 128)
        self.assertNotIn('PRIVATE_PATH', json.dumps(value))
        self.assertNotIn('TOKEN_SECRET', json.dumps(value))
        self.assertEqual(value['query_failures'][0]['project_ids'], ['p1'])

    def test_git_query_completes_while_watch_stdin_remains_open(self):
        script = '''import json, sys
from pathlib import Path
from threading import Thread
sys.path.insert(0, sys.argv[1])
from local_activity_monitor.worktree_info import WorktreeCollector
Thread(target=lambda: sys.stdin.readline(), daemon=True).start()
records, health = WorktreeCollector(Path(sys.argv[2])).git_list(Path(sys.argv[3]), 2)
print(json.dumps({"health": health, "items": len(records or [])}))
'''
        process = subprocess.Popen([sys.executable, '-I', '-c', script, str(Path(__file__).resolve().parents[1]/'src'),
                                    str(self.home), str(self.repo)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            process.wait(timeout=6)
            self.assertEqual(process.returncode, 0, process.stderr.read(4096).decode('utf-8', 'replace'))
            result = json.loads(process.stdout.read())
            self.assertEqual(result, {'health': 'ok', 'items': 2})
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=3)
            for stream in (process.stdin, process.stdout, process.stderr):
                stream.close()

    def test_temporary_file_error_remains_diagnostic_without_private_message(self):
        with patch('local_activity_monitor.worktree_info.tempfile.TemporaryFile', side_effect=PermissionError(13, 'PRIVATE_PATH')):
            value = self.collector.refresh(self.projects, [], {})
        self.assertIsNone(value['total'])
        self.assertEqual(value['query_failures'][0]['reason'], 'temporary_file_unavailable')
        self.assertEqual(value['query_failures'][0]['error_code'], 13)
        self.assertNotIn('PRIVATE_PATH', json.dumps(value))

    def test_dashboard_worktree_endpoint_preserves_path_boundary_and_observation_switch(self):
        sessions = self.home/"sessions"
        sessions.mkdir()
        (sessions/f"rollout-{THREAD}.jsonl").write_text(json.dumps({"type": "session_meta", "timestamp": "2026-10-06T00:00:00Z", "payload": {"id": THREAD, "cwd": str(self.linked)}})+"\n", encoding="utf-8")
        with closing(sqlite3.connect(self.home/"state_5.sqlite")) as database:
            database.executescript("CREATE TABLE threads(id TEXT,title TEXT,project_id TEXT,cwd TEXT); CREATE TABLE projects(id TEXT,name TEXT); CREATE TABLE project_roots(project_id TEXT,path TEXT);")
            database.execute("INSERT INTO threads VALUES(?,?,?,?)", (THREAD, "Fixture", "p1", str(self.linked)))
            database.execute("INSERT INTO projects VALUES(?,?)", ("p1", "Fixture"))
            database.execute("INSERT INTO project_roots VALUES(?,?)", ("p1", str(self.repo)))
            database.commit()
        dashboard = Dashboard(self.home, codex=True)
        dashboard.refresh()
        all_data = dashboard.snapshot("all")
        linked = next(item for item in all_data["codex"]["worktrees"]["items"] if item["managed"])
        self.assertEqual(linked["thread_ids"], [THREAD])
        self.assertNotIn("cwd", all_data["codex"]["threads"][0])
        self.assertEqual(dashboard.snapshot("1h")["codex"]["worktrees"], all_data["codex"]["worktrees"])
        request_handler = handler(dashboard, 8787)
        def request(query):
            current = object.__new__(request_handler)
            current.headers = {"Host": "127.0.0.1:8787"}
            current.path = query
            responses = []
            current.reply = lambda code, body, *args, **kwargs: responses.append((code, body))
            current.do_GET()
            return responses[0]
        query = "/api/codex/worktree?id="+linked["id"]
        self.assertEqual(json.loads(request(query)[1])["path"], str(self.linked))
        for suffix in ("&path=../outside", "&id="+linked["id"]):
            self.assertEqual(request(query+suffix)[0], 400)
        self.assertEqual(request("/api/codex/worktree?id=../outside")[0], 400)
        self.assertEqual(request("/api/codex/worktree?id="+"f"*24)[0], 409)
        dashboard.set_settings({"observations": {"worktrees": False}})
        self.assertEqual(request(query)[0], 409)
        self.assertEqual(dashboard.snapshot("all")["codex"]["worktrees"]["health"], "disabled")
        with patch.object(dashboard.worktrees, "read", side_effect=AssertionError("disabled Codex read")):
            dashboard.set_settings({"observations": {"codex": False, "worktrees": True}})


class WorktreeFormatTests(unittest.TestCase):
    def test_porcelain_null_records_keep_flags_without_free_form_reasons(self):
        rows = parse_worktrees(("worktree /repo with spaces\0HEAD "+"a"*40+"\0branch refs/heads/feature/測試\0\0worktree /linked\0detached\0locked PRIVATE_SECRET\0prunable PRIVATE_REASON\0\0").encode())
        self.assertEqual(rows[0]["branch"], "feature/測試")
        self.assertTrue(rows[1]["locked"])
        self.assertTrue(rows[1]["prunable"])
        self.assertNotIn("PRIVATE", repr(rows))


if __name__ == "__main__":
    unittest.main()
