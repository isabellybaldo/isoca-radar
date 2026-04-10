from fastapi import Request
from slowapi import Limiter


def _get_real_ip(request: Request) -> str:
    xff = request.headers.get("X-Forwarded-For")
    if xff:
        return xff.split(",")[0].strip()
    return request.client.host


limiter = Limiter(key_func=_get_real_ip)
