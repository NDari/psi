"""The /users list endpoint."""
from pager import paginate

USERS = [{"id": i, "name": f"user{i:03d}"} for i in range(1, 46)]

MAX_PER_PAGE = 100


def _int_param(query, name, default):
    raw = query.get(name, default)
    try:
        return int(raw)
    except (TypeError, ValueError):
        raise ValueError(f"{name} must be a number") from None


def list_users(query):
    """Handle GET /users?page=N&per_page=M. Returns (status, body dict)."""
    try:
        page = _int_param(query, "page", 1)
        per_page = min(_int_param(query, "per_page", 20), MAX_PER_PAGE)
        return 200, paginate(USERS, page, per_page).to_dict()
    except ValueError as e:
        return 400, {"error": str(e)}
