# Adapters

Adapters in **Rupā** handle interactions with external systems and tools. They make the app flexible and easy to extend.

## Database Adapter
- Supports multiple database providers (e.g., SQLite, PostgreSQL, MySQL).
- Lets you switch databases easily without changing the core logic.
- Configured using `RUPA_DATABASE_PATH` or other settings.

## Platform CLI Adapter
- Uses system commands to get information (e.g., IP details, WireGuard status).
- Works on Linux to ensure compatibility with common tools.
- Handles platform-specific logic, like running `wg show` or `ip` commands.
