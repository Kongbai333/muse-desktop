"""Debug binary integration test. Uses disposable synthetic profiles and local fixture routes only."""
import argparse
import ctypes
from ctypes import wintypes
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument('--proxy', action='store_true')
parser.add_argument('--dark', action='store_true')
args = parser.parse_args()
if sys.platform != 'win32':
    raise SystemExit('This runner verifies Windows only; it is not a macOS test.')
repo = Path(__file__).resolve().parents[1]
(repo / '.runtime').mkdir(exist_ok=True)
root = Path(tempfile.mkdtemp(prefix='encrypted-transfer-native-', dir=repo / '.runtime'))
if args.dark:
    (root / 'display.json').write_text(json.dumps({'theme':'dark','page_zoom':1.0}), encoding='utf-8')
startup = subprocess.STARTUPINFO()
startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
startup.wShowWindow = subprocess.SW_HIDE
mode = '--proxy-smoke-test' if args.proxy else '--smoke-test'
process = subprocess.Popen([str(repo / 'target/debug/muse-desktop.exe'), mode,
                            '--encrypted-transfer-smoke-test', '--data-dir', str(root)], startupinfo=startup)
user32 = ctypes.windll.user32
callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
user32.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.SetWindowPos.argtypes = [wintypes.HWND, wintypes.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_uint]
windows = []
@callback_type
def find_window(hwnd, _):
    pid = wintypes.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    title = ctypes.create_unicode_buffer(128)
    user32.GetWindowTextW(hwnd, title, 128)
    if pid.value == process.pid and title.value == 'Muse':
        windows.append(hwnd)
    return True

capture = None
shown = False
deadline = time.monotonic() + 85
try:
    while process.poll() is None and time.monotonic() < deadline:
        if not shown:
            user32.EnumWindows(find_window, 0)
            if windows:
                user32.SetWindowPos(windows[0], wintypes.HWND(1), 0, 0, 0, 0, 0x0010 | 0x0002 | 0x0001 | 0x0040)
                shown = True
        if capture is None and (root / 'transfer-ui-preview-ready').exists():
            capture = subprocess.Popen(['powershell', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
                str(repo / 'scripts/capture-window.ps1'), '-ProcessId', str(process.pid),
                '-OutputPath', str(root / 'transfer-preview.png')], stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE, creationflags=subprocess.CREATE_NO_WINDOW)
        time.sleep(0.1)
    if process.poll() is None:
        raise RuntimeError('Owned diagnostic process timed out')
    if capture is not None:
        _, capture_error = capture.communicate(timeout=10)
        if capture.returncode:
            print(capture_error.decode(errors='replace'), file=sys.stderr)
    report_file = root / 'smoke-result.json'
    if not report_file.exists():
        crash = root / 'crash.log'
        if crash.exists():
            print(crash.read_text(errors='replace'))
        raise RuntimeError(f'Owned diagnostic process exited without a report: {root}')
    report = json.loads(report_file.read_text(encoding='utf-8'))
    print(json.dumps(report, ensure_ascii=False, indent=2))
    print(f'Synthetic diagnostic evidence: {root}')
    required = ['passed', 'password_confirmation', 'password_input_cleared', 'wrong_password_atomic',
        'preview_before_profiles', 'preview_confirmation_required', 'cancelled_vault_not_activated',
        'latest_workspace_preserved', 'native_profile_isolation', 'native_scoped_cookie_readback',
        'shared_static_proxy_binding', 'activation_after_commit', 'diagnostic_route_used', 'encrypted_reexport']
    for key in required:
        assert report.get(key) is True, f'Failed integration check: {key}'
    assert report['real_service_identity_verified'] is False
    assert report['file_picker_result_injected'] is True
    assert process.returncode == 0
finally:
    if process.poll() is None:
        process.terminate()  # Only the child process created by this runner.
        process.wait(timeout=10)
