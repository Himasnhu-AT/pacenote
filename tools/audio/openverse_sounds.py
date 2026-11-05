#!/usr/bin/env python3
"""Search and download openly licensed audio from Openverse (api.openverse.org).

Standard library only. Raw downloads are sources, so they go under `assets_src/` (gitignored, like
every other download); `tools/audio/cook_sfx.py` turns them into the game's files in
`assets/sounds/sfx/` and writes the credits.

Pacenote only ships CC0 and CC BY (assets_src/SOURCES.md), and every sound is trimmed and
re-levelled, so by default the search asks for licences that allow commercial use and modification,
limited to CC0 / CC BY. Override with --license / --any-use if you only want to listen.

One-off search:
    python3 -I tools/audio/openverse_sounds.py "camera shutter" -n 5 --category sound_effect --length shortest
    python3 -I tools/audio/openverse_sounds.py "crowd cheering" --pack finish_crowd --dry-run

Everything the game wants (tools/audio/sfx_wishlist.json):
    python3 -I tools/audio/openverse_sounds.py --wishlist             # download what's missing
    python3 -I tools/audio/openverse_sounds.py --wishlist --dry-run   # just list candidates
    python3 -I tools/audio/openverse_sounds.py --wishlist --only radar_shutter,finish_crowd

Optional (higher rate limits): register an app (https://api.openverse.org/v1/#tag/auth) and set
OPENVERSE_CLIENT_ID and OPENVERSE_CLIENT_SECRET.

Every download is recorded in <pack dir>/credits.json and CREDITS.md (title, author, licence,
landing page). CC BY needs attribution: cook_sfx.py copies it into assets/credits.txt.
"""
import argparse
import json
import os
import re
import ssl
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.join(ROOT, 'assets_src', 'audio', 'openverse')
WISHLIST = os.path.join(ROOT, 'tools', 'audio', 'sfx_wishlist.json')
API = 'https://api.openverse.org/v1'
UA = 'pacenote-asset-tools/1.0 (+https://openverse.org)'
# What the game may ship (assets_src/SOURCES.md): CC0, public domain mark, CC BY.
SHIPPABLE = 'cc0,pdm,by'


def tls_context():
    """Python from python.org on macOS ships without root certificates until "Install
    Certificates.command" runs; fall back to the system bundle so the script works as is."""
    ctx = ssl.create_default_context()
    if not ctx.get_ca_certs():
        for cafile in ('/etc/ssl/cert.pem', '/opt/homebrew/etc/openssl@3/cert.pem',
                       '/usr/local/etc/openssl@3/cert.pem', '/etc/ssl/certs/ca-certificates.crt'):
            if os.path.exists(cafile):
                ctx.load_verify_locations(cafile)
                break
    return ctx


TLS = tls_context()


def http(url, data=None, headers=None, retries=4):
    h = {'User-Agent': UA, 'Accept': 'application/json'}
    h.update(headers or {})
    for attempt in range(retries):
        req = urllib.request.Request(url, data=data, headers=h)
        try:
            with urllib.request.urlopen(req, timeout=60, context=TLS) as r:
                return r.read(), r.headers
        except urllib.error.HTTPError as e:
            # Cloudflare sometimes answers a first request with a 403 "Just a moment" page.
            if e.code == 403 and attempt < retries - 1 and 'json' in h.get('Accept', ''):
                print('  403 from the API front (bot check), retrying...', file=sys.stderr)
                time.sleep(8 * (attempt + 1))
                continue
            if e.code == 429 and attempt < retries - 1:
                wait = int(e.headers.get('Retry-After') or 10 * (attempt + 1))
                print(f'  rate limited, waiting {wait}s...', file=sys.stderr)
                time.sleep(wait)
                continue
            body = e.read()[:300].decode('utf-8', 'replace')
            raise RuntimeError(f'HTTP {e.code} for {url}\n{body}')
        except urllib.error.URLError as e:
            if attempt < retries - 1:
                time.sleep(3)
                continue
            raise RuntimeError(f'network error for {url}: {e.reason}')


def get_token():
    cid, secret = os.getenv('OPENVERSE_CLIENT_ID'), os.getenv('OPENVERSE_CLIENT_SECRET')
    if not (cid and secret):
        return None
    data = urllib.parse.urlencode({'grant_type': 'client_credentials',
                                   'client_id': cid, 'client_secret': secret}).encode()
    body, _ = http(f'{API}/auth_tokens/token/', data=data,
                   headers={'Content-Type': 'application/x-www-form-urlencoded'})
    return json.loads(body)['access_token']


def search(q, token):
    """`q`: dict with query, n, category, length, ext, source, license, any_use."""
    params = {'q': q['query'], 'page_size': min(max(q['n'] * 3, 5), 50)}
    for key, api_key in (('category', 'category'), ('length', 'length'), ('source', 'source'),
                         ('ext', 'extension')):
        if q.get(key):
            params[api_key] = q[key]
    if not q.get('any_use'):
        params['license_type'] = 'commercial,modification'
        params['license'] = q.get('license') or SHIPPABLE
    elif q.get('license'):
        params['license'] = q['license']
    headers = {'Authorization': f'Bearer {token}'} if token else {}
    body, _ = http(f'{API}/audio/?{urllib.parse.urlencode(params)}', headers=headers)
    return json.loads(body)


def safe_name(s, limit=60):
    s = re.sub(r'[^\w\- ]+', '', s or 'untitled').strip().replace(' ', '_')
    return (s or 'untitled')[:limit]


def ext_for(item):
    path = urllib.parse.urlparse(item.get('url') or '').path
    m = re.search(r'\.(mp3|wav|ogg|flac|m4a|aac|opus|aiff?)$', path, re.I)
    if m:
        return m.group(1).lower()
    ft = (item.get('filetype') or '').lower()
    return re.sub(r'\d+$', '', ft) or 'mp3'  # the API sometimes reports e.g. "mp32"


def max_ms_ok(item, q):
    lim = q.get('max_s')
    return not lim or (item.get('duration') or 0) <= lim * 1000


def write_credits(out, credits):
    with open(os.path.join(out, 'credits.json'), 'w') as f:
        json.dump(credits, f, indent=2)
    with open(os.path.join(out, 'CREDITS.md'), 'w') as f:
        f.write('# Audio credits\n\n')
        for c in credits:
            f.write(f"- **{c['file']}** - {c.get('attribution') or c['title']}  \n"
                    f"  Source: {c.get('landing_page')}\n")


def fetch(q, out, token, dry_run):
    """Download up to q['n'] new files for one query into `out`. Returns how many it got."""
    data = search(q, token)
    results = [r for r in data.get('results', []) if r.get('url') and max_ms_ok(r, q)]
    print(f"{data.get('result_count', 0)} matches for '{q['query']}' -> {os.path.relpath(out, ROOT)}")
    if not results:
        return 0
    credits_path = os.path.join(out, 'credits.json')
    credits = json.load(open(credits_path)) if os.path.exists(credits_path) else []
    have = {c['id'] for c in credits}
    got = 0
    for r in results:
        if got >= q['n']:
            break
        dur = f"{(r.get('duration') or 0) / 1000:.1f}s"
        lic = f"{r.get('license', '?').upper()} {r.get('license_version') or ''}".strip()
        line = f"  - {r.get('title')} by {r.get('creator') or 'unknown'} [{lic}, {dur}, {r.get('source')}]"
        if dry_run:
            print(line)
            got += 1
            continue
        if r['id'] in have:
            print(f"  skip (already downloaded): {r.get('title')}")
            continue
        os.makedirs(out, exist_ok=True)
        fname = f"{safe_name(r.get('title'))}_{r['id'][:8]}.{ext_for(r)}"
        try:
            blob, _ = http(r['url'], headers={'Accept': '*/*'}, retries=2)
        except RuntimeError as e:
            print(f"  failed: {r.get('title')} ({str(e).splitlines()[0]})")
            continue
        with open(os.path.join(out, fname), 'wb') as f:
            f.write(blob)
        print(line + f' -> {fname}')
        credits.append({'id': r['id'], 'file': fname, 'title': r.get('title'),
                        'creator': r.get('creator'), 'license': r.get('license'),
                        'license_version': r.get('license_version'),
                        'license_url': r.get('license_url'), 'source': r.get('source'),
                        'landing_page': r.get('foreign_landing_url'),
                        'attribution': r.get('attribution'),
                        'duration_ms': r.get('duration'),
                        'downloaded': time.strftime('%Y-%m-%d')})
        got += 1
    if not dry_run and got:
        write_credits(out, credits)
    return got


def load_wishlist(only):
    with open(WISHLIST) as f:
        packs = json.load(f)['packs']
    if only:
        want = set(only.split(','))
        unknown = want - {p['pack'] for p in packs}
        if unknown:
            raise SystemExit(f"not in {os.path.relpath(WISHLIST, ROOT)}: {', '.join(sorted(unknown))}")
        packs = [p for p in packs if p['pack'] in want]
    return packs


def main():
    ap = argparse.ArgumentParser(description='Search and download Openverse audio for Pacenote')
    ap.add_argument('query', nargs='?', help='search words (omit with --wishlist)')
    ap.add_argument('-n', type=int, default=3, help='files to download (default 3)')
    ap.add_argument('--pack', help='folder name under assets_src/audio/openverse (default: from the query)')
    ap.add_argument('-o', '--out', help='output folder (overrides --pack)')
    ap.add_argument('--category', choices=['sound_effect', 'music', 'podcast', 'audiobook', 'news', 'pronunciation'])
    ap.add_argument('--length', choices=['shortest', 'short', 'medium', 'long'], help='shortest = under 30 s')
    ap.add_argument('--max-s', type=float, help='skip results longer than this many seconds')
    ap.add_argument('--ext', help='file extension filter, e.g. wav, ogg, flac, mp3')
    ap.add_argument('--source', help='e.g. freesound, wikimedia_audio, ccmixter, jamendo')
    ap.add_argument('--license', help=f'licence filter (default {SHIPPABLE})')
    ap.add_argument('--any-use', action='store_true',
                    help='drop the commercial + modification filter (listening only: not shippable)')
    ap.add_argument('--wishlist', action='store_true', help=f'fetch every pack in {os.path.relpath(WISHLIST, ROOT)}')
    ap.add_argument('--only', help='with --wishlist: comma-separated pack names')
    ap.add_argument('--dry-run', action='store_true', help="list results, don't download")
    args = ap.parse_args()

    token = get_token()
    if args.wishlist:
        total = 0
        for p in load_wishlist(args.only):
            out = os.path.join(SRC, p['pack'])
            existing = len(json.load(open(os.path.join(out, 'credits.json')))) if os.path.exists(os.path.join(out, 'credits.json')) else 0
            need = p.get('n', 3) - existing
            if need <= 0 and not args.dry_run:
                print(f"{p['pack']}: have {existing}, skipping")
                continue
            q = dict(p, n=max(need, 1) if not args.dry_run else p.get('n', 3))
            try:
                total += fetch(q, out, token, args.dry_run)
            except RuntimeError as e:
                print(f"  {p['pack']}: {e}", file=sys.stderr)
            time.sleep(1.0)  # be polite to the anonymous rate limit
        print(f"\n{'Listed' if args.dry_run else 'Downloaded'} {total} file(s). Next: python3 -I tools/audio/cook_sfx.py")
        return
    if not args.query:
        ap.error('give a query, or --wishlist')
    out = args.out or os.path.join(SRC, args.pack or safe_name(args.query.lower(), 40))
    q = {'query': args.query, 'n': args.n, 'category': args.category, 'length': args.length,
         'ext': args.ext, 'source': args.source, 'license': args.license,
         'any_use': args.any_use, 'max_s': args.max_s}
    try:
        got = fetch(q, out, token, args.dry_run)
    except RuntimeError as e:
        raise SystemExit(str(e))
    if not args.dry_run:
        print(f'\nDownloaded {got} file(s) to {os.path.relpath(out, ROOT)}/ (credits in CREDITS.md)')


if __name__ == '__main__':
    main()
