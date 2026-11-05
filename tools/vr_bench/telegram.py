"""Progress reports to a Telegram bot (Bot API sendMessage / sendPhoto / sendDocument).

The token and chat id live OUTSIDE the repository, in %LOCALAPPDATA%\\vr_bench\\telegram.json:
    {"token": "<bot token>", "chat_id": "<chat id>"}
Without that file every call is a no-op that returns False, so a bench run never fails on reporting.

    py -3.13 -B tools/vr_bench/telegram.py "text"            # one message
    py -3.13 -B tools/vr_bench/telegram.py --photo a.jpg "caption"
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import uuid

CONFIG = Path(os.environ.get('LOCALAPPDATA', '.')) / 'vr_bench' / 'telegram.json'
LIMIT = 4000  # Telegram's message limit is 4096 characters


def _config():
    try:
        value = json.loads(CONFIG.read_text(encoding='utf-8'))
        return value['token'], str(value['chat_id'])
    except (OSError, ValueError, KeyError):
        return None, None


def _post(method, fields, files=None, timeout=60):
    """One Bot API call through curl.exe: it verifies TLS against the Windows certificate store (Schannel), which
    this machine's HTTPS inspection root is in and Python's bundle is not. The URL (it holds the token) goes to curl
    on stdin as a config file, not on the command line."""
    token, chat = _config()
    if not token:
        return False
    fields = dict(fields, chat_id=chat)
    config = [f'url = "https://api.telegram.org/bot{token}/{method}"', 'silent', 'show-error',
              f'max-time = {int(timeout)}']
    temporary = None
    if files:
        for key, value in fields.items():
            text = str(value).replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n')
            config.append(f'form-string = "{key}={text}"')
        for key, path in files.items():
            config.append(f'form = "{key}=@{Path(path).resolve().as_posix()}"')
    else:
        temporary = Path(os.environ.get('TEMP', '.')) / f'vr_bench_tg_{uuid.uuid4().hex}.json'
        temporary.write_text(json.dumps(fields), encoding='utf-8')
        config += ['header = "Content-Type: application/json"', f'data-binary = "@{temporary.as_posix()}"']
    try:
        result = subprocess.run(['curl.exe', '-K', '-'], input='\n'.join(config) + '\n', capture_output=True,
                                text=True, encoding='utf-8', errors='replace', timeout=timeout + 10)
        reply = json.loads(result.stdout or '{}')
        if not reply.get('ok'):
            print(f'[telegram] {method} failed: {reply.get("description") or result.stderr.strip()}', file=sys.stderr)
        return bool(reply.get('ok'))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f'[telegram] {method} failed: {error}', file=sys.stderr)
        return False
    finally:
        if temporary:
            temporary.unlink(missing_ok=True)


def send(text):
    """Sends text in as many messages as it takes. Returns True when every part was accepted."""
    ok = True
    parts = [text[i:i + LIMIT] for i in range(0, len(text), LIMIT)] or ['']
    for part in parts:
        ok = _post('sendMessage', {'text': part, 'disable_web_page_preview': True}) and ok
    return ok


def photo(path, caption=''):
    return _post('sendPhoto', {'caption': caption[:1000]}, {'photo': path})


def document(path, caption=''):
    return _post('sendDocument', {'caption': caption[:1000]}, {'document': path})


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('text', nargs='?', default='')
    parser.add_argument('--photo', type=Path)
    parser.add_argument('--document', type=Path)
    args = parser.parse_args()
    if args.photo:
        ok = photo(args.photo, args.text)
    elif args.document:
        ok = document(args.document, args.text)
    else:
        ok = send(args.text)
    print('sent' if ok else 'NOT sent')
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main())
