#!/usr/bin/env python3
"""
⚡ AUTOMATIC ROTATING PROXY FORWARDER & SCRAPER
================================================
Автоматический ротатор прокси как в Telegram-краулерах:
1. Автоматически скачивает свежие списки прокси из проверенных репозиториев (ProxyScrape, TheSpeedX, monosans, sunny9577).
2. Тестирует их в пуле потоков на доступность и скорость.
3. Поднимает локальный HTTP-прокси сервер на 127.0.0.1:8888 с автоматической ротацией (каждый запрос идет через новый живой прокси).
4. Любой скрипт, бот или краулер может использовать HTTP_PROXY=http://127.0.0.1:8888.
"""

import sys
import time
import socket
import select
import threading
import urllib.request
import json
import random
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import urlparse

# Порт локального ротатора
LOCAL_PORT = 8888
LOCAL_HOST = "127.0.0.1"

# Источники публичных прокси
PROXY_SOURCES = [
    ("sunny9577", "https://raw.githubusercontent.com/sunny9577/proxy-scraper/master/proxies.json"),
    ("proxyscrape_us", "https://api.proxyscrape.com/v2/?request=displayproxies&protocol=http&timeout=5000&country=US&ssl=yes"),
    ("thespeedx", "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/http.txt"),
    ("monosans", "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies/http.txt"),
]

class ProxyPool:
    def __init__(self):
        self.lock = threading.Lock()
        self.alive_proxies = []
        self.current_idx = 0
        self.is_running = True

    def add_proxy(self, proxy_addr):
        with self.lock:
            if proxy_addr not in self.alive_proxies:
                self.alive_proxies.append(proxy_addr)

    def remove_proxy(self, proxy_addr):
        with self.lock:
            if proxy_addr in self.alive_proxies:
                self.alive_proxies.remove(proxy_addr)

    def get_next(self):
        with self.lock:
            if not self.alive_proxies:
                return None
            self.current_idx = (self.current_idx + 1) % len(self.alive_proxies)
            return self.alive_proxies[self.current_idx]

    def count(self):
        with self.lock:
            return len(self.alive_proxies)

POOL = ProxyPool()

def fetch_raw_proxies():
    """Скачивает списки прокси из открытых источников."""
    raw = set()
    for name, url in PROXY_SOURCES:
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
            with urllib.request.urlopen(req, timeout=8) as resp:
                content = resp.read().decode("utf-8", errors="ignore")
                if url.endswith(".json"):
                    data = json.loads(content)
                    for item in data:
                        if isinstance(item, dict) and "ip" in item and "port" in item:
                            raw.add(f"{item['ip']}:{item['port']}")
                else:
                    for line in content.splitlines():
                        line = line.strip()
                        if line and ":" in line and not line.startswith("#"):
                            raw.add(line)
            print(f"[+] Источник '{name}' загружен (всего кандидатов: {len(raw)})")
        except Exception as e:
            print(f"[-] Ошибка загрузки источника '{name}': {e}")
    return list(raw)

def test_proxy(proxy_str, test_target="https://api.ipify.org"):
    """Проверяет скорость и работоспособность прокси."""
    proxy_handler = urllib.request.ProxyHandler({
        'http': f"http://{proxy_str}",
        'https': f"http://{proxy_str}"
    })
    opener = urllib.request.build_opener(proxy_handler)
    req = urllib.request.Request(test_target, headers={"User-Agent": "Mozilla/5.0"})
    t0 = time.time()
    try:
        with opener.open(req, timeout=4) as resp:
            if resp.status == 200:
                elapsed_ms = int((time.time() - t0) * 1000)
                return True, elapsed_ms
    except Exception:
        pass
    return False, 0

def background_health_worker():
    """Фоновый поток: ищет и поддерживает пул живых прокси."""
    print("[*] Запущен фоновый сканер прокси...")
    while POOL.is_running:
        candidates = fetch_raw_proxies()
        random.shuffle(candidates)
        print(f"[*] Проверка пачки из {min(150, len(candidates))} прокси...")

        threads = []
        def check_and_add(p):
            ok, ms = test_proxy(p)
            if ok:
                POOL.add_proxy(p)
                print(f"   ✓ [ПРОКСИ НАЙДЕН] {p} (пинг: {ms}ms) | В пуле: {POOL.count()}")

        for p in candidates[:150]:
            t = threading.Thread(target=check_and_add, args=(p,))
            t.daemon = True
            t.start()
            threads.append(t)
            time.sleep(0.05)

        for t in threads:
            t.join(timeout=5)

        print(f"[✓] Цикл проверки завершен. Активных прокси в пуле: {POOL.count()}")
        time.sleep(180)  # Повторная проверка каждые 3 минуты

class RotatingProxyHandler(BaseHTTPRequestHandler):
    """Локальный прокси-сервер с ротацией для входящих HTTP/HTTPS (CONNECT) запросов."""

    def do_CONNECT(self):
        """Обработка HTTPS туннеля (CONNECT)."""
        upstream_proxy = POOL.get_next()
        if not upstream_proxy:
            self.send_error(503, "No alive upstream proxies available in pool")
            return

        u_host, u_port = upstream_proxy.split(":")
        u_port = int(u_port)

        try:
            upstream_sock = socket.create_connection((u_host, u_port), timeout=6)
            # Отправляем CONNECT запрос внешнему прокси
            connect_req = f"CONNECT {self.path} HTTP/1.1\r\nHost: {self.path}\r\nProxy-Connection: Keep-Alive\r\n\r\n"
            upstream_sock.sendall(connect_req.encode())

            # Читаем ответ от прокси
            resp = upstream_sock.recv(4096)
            if b"200" not in resp:
                POOL.remove_proxy(upstream_proxy)
                self.send_error(502, f"Upstream proxy {upstream_proxy} failed CONNECT")
                upstream_sock.close()
                return

            self.send_response(200, "Connection Established")
            self.end_headers()

            # Двунаправленный проброс трафика (Туннель)
            self.pipe_sockets(self.connection, upstream_sock)
        except Exception as e:
            POOL.remove_proxy(upstream_proxy)
            self.send_error(502, f"Proxy connect error: {e}")

    def do_GET(self):
        self.forward_http()

    def do_POST(self):
        self.forward_http()

    def forward_http(self):
        """Прямой проброс обычных HTTP запросов."""
        upstream_proxy = POOL.get_next()
        if not upstream_proxy:
            self.send_error(503, "No proxies available")
            return

        parsed = urlparse(self.path)
        url = self.path if parsed.netloc else f"http://{self.headers.get('Host', '')}{self.path}"

        try:
            content_len = int(self.headers.get('Content-Length', 0))
            body = self.rfile.read(content_len) if content_len > 0 else None

            proxy_handler = urllib.request.ProxyHandler({'http': f"http://{upstream_proxy}"})
            opener = urllib.request.build_opener(proxy_handler)
            req = urllib.request.Request(url, data=body, method=self.command)
            for k, v in self.headers.items():
                if k.lower() not in ('host', 'proxy-connection'):
                    req.add_header(k, v)

            with opener.open(req, timeout=8) as resp:
                self.send_response(resp.status)
                for k, v in resp.headers.items():
                    self.send_header(k, v)
                self.end_headers()
                self.wfile.write(resp.read())
        except Exception as e:
            POOL.remove_proxy(upstream_proxy)
            self.send_error(502, f"Forwarding error: {e}")

    def pipe_sockets(self, client_sock, target_sock):
        socks = [client_sock, target_sock]
        try:
            while True:
                r, _, _ = select.select(socks, [], socks, 15)
                if not r:
                    break
                for s in r:
                    data = s.recv(8192)
                    if not data:
                        return
                    if s is client_sock:
                        target_sock.sendall(data)
                    else:
                        client_sock.sendall(data)
        except Exception:
            pass
        finally:
            client_sock.close()
            target_sock.close()

def run_server():
    server = HTTPServer((LOCAL_HOST, LOCAL_PORT), RotatingProxyHandler)
    print("\n" + "=" * 65)
    print(f"🚀 [ROTATING PROXY FORWARDER] Активен на http://{LOCAL_HOST}:{LOCAL_PORT}")
    print("   Поддерживает: HTTP / HTTPS (CONNECT), авто-ротация на каждый запрос")
    print(f"   Для использования установите: HTTP_PROXY=http://{LOCAL_HOST}:{LOCAL_PORT}")
    print("=" * 65 + "\n")
    server.serve_forever()

if __name__ == "__main__":
    t = threading.Thread(target=background_health_worker)
    t.daemon = True
    t.start()

    run_server()
