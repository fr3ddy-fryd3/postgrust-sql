# PostgrustSQL Installation & Configuration Guide

Complete guide for installing, configuring, and running PostgrustSQL v2.2.0.

## Table of Contents

- [Installation Methods](#installation-methods)
  - [From Source](#from-source)
  - [Arch Linux (PKGBUILD)](#arch-linux)
  - [Debian/Ubuntu (.deb)](#debianubuntu)
- [Configuration](#configuration)
  - [Configuration Priority](#configuration-priority)
  - [Configuration File](#configuration-file)
  - [Environment Variables](#environment-variables)
  - [CLI Arguments](#cli-arguments)
- [Running the Server](#running-the-server)
  - [Manual Start](#manual-start)
  - [Systemd Service](#systemd-service)
- [Client Tools](#client-tools)
- [Backup & Restore](#backup--restore)
- [Troubleshooting](#troubleshooting)

---

## Installation Methods

### From Source

**Requirements:**
- Rust 1.70+ (`rustc --version`)
- Cargo package manager
- Git

**Steps:**

```bash
# 1. Clone repository
git clone https://github.com/fr3ddy-fryd3/postgrust-sql.git
cd postgrust-sql

# 2. Build release binaries
make release
# or: cargo build --release

# 3. Install system-wide (requires sudo)
sudo make install

# 4. Verify installation
postgrustql --version
pgr_cli --help
```

**Installation paths:**
- Binaries: `/usr/local/bin/{postgrustql,pgr_cli,pgr_dump,pgr_restore}`
- Config: `/etc/postgrustsql/postgrustsql.toml`
- Data: `/var/lib/postgrustsql/data`

**Uninstall:**
```bash
sudo make uninstall
```

---

### Arch Linux

**Using PKGBUILD:**

```bash
# 1. Build package
makepkg -si

# 2. Or install from AUR (when available)
yay -S postgrustsql
```

**Post-installation:**
```bash
# Create postgres user (if needed)
sudo useradd -r -M -s /bin/false postgres
sudo chown -R postgres:postgres /var/lib/postgrustsql

# Enable and start service
sudo systemctl enable postgrustsql
sudo systemctl start postgrustsql

# Check status
sudo systemctl status postgrustsql
```

**Uninstall:**
```bash
sudo pacman -R postgrustsql
# or purge with data:
sudo pacman -Rns postgrustsql
sudo rm -rf /var/lib/postgrustsql
```

---

### Debian/Ubuntu

**Build .deb package:**

```bash
# 1. Install build dependencies
sudo apt install build-essential debhelper cargo rustc

# 2. Build package
dpkg-buildpackage -us -uc -b

# 3. Install
sudo dpkg -i ../postgrustsql_2.2.0-1_amd64.deb
sudo apt install -f  # Fix dependencies if needed
```

**Post-installation:**
```bash
# Service is auto-configured, just start it
sudo systemctl enable postgrustsql
sudo systemctl start postgrustsql

# Check status
sudo systemctl status postgrustsql
```

**Uninstall:**
```bash
# Remove but keep data
sudo apt remove postgrustsql

# Purge (removes data)
sudo apt purge postgrustsql
```

---

## Configuration

### Configuration Priority

PostgrustSQL uses a **layered configuration system** with the following priority (highest to lowest):

```
1. CLI Arguments   (--host, --port, etc.)
   ↓
2. Environment Variables (POSTGRUSTQL_*)
   ↓
3. Config File     (/etc/postgrustsql/postgrustsql.toml or ./postgrustsql.toml)
   ↓
4. Built-in Defaults
```

**Example:** If you set `POSTGRUSTQL_PORT=5433` but config file has `port = 5432`, the server will use **5433**.

---

### Configuration File

**Locations (checked in order):**
1. `/etc/postgrustsql/postgrustsql.toml` (system-wide)
2. `./postgrustsql.toml` (current directory)

**Example config** (`/etc/postgrustsql/postgrustsql.toml`):

```toml
# PostgrustSQL Server Configuration
# Priority: ENV variables > this file > defaults

# Server settings
host = "127.0.0.1"
port = 5432
data_dir = "/var/lib/postgrustsql/data"
initdb = true

# Authentication (reserved for future use)
user = "postgres"
password = "postgres"
database = "postgres"
```

**Parameters:**

| Parameter  | Type    | Default                       | Description                          |
|------------|---------|-------------------------------|--------------------------------------|
| `host`     | String  | `"127.0.0.1"`                 | Server bind address                  |
| `port`     | Integer | `5432`                        | Server port                          |
| `data_dir` | String  | `"/var/lib/postgrustsql/data"`| Database storage directory           |
| `initdb`   | Boolean | `true`                        | Initialize database on first run     |
| `user`     | String  | `"postgres"`                  | Superuser name (future auth support) |
| `password` | String  | `"postgres"`                  | Superuser password (future)          |
| `database` | String  | `"postgres"`                  | Default database name                |

**Edit config:**
```bash
sudo vim /etc/postgrustsql/postgrustsql.toml
sudo systemctl restart postgrustsql
```

---

### Environment Variables

**Server environment variables:**

| Variable                  | Example                        | Description              |
|---------------------------|--------------------------------|--------------------------|
| `POSTGRUSTQL_HOST`        | `127.0.0.1`                    | Server bind address      |
| `POSTGRUSTQL_PORT`        | `5432`                         | Server port              |
| `POSTGRUSTQL_DATA_DIR`    | `/var/lib/postgrustsql/data`   | Data directory           |
| `POSTGRUSTQL_INITDB`      | `true`                         | Initialize DB on start   |
| `POSTGRUSTQL_USER`        | `postgres`                     | Superuser (future)       |
| `POSTGRUSTQL_PASSWORD`    | `postgres`                     | Password (future)        |
| `POSTGRUSTQL_DATABASE`    | `postgres`                     | Default database         |

**Examples:**

```bash
# Run server on custom port
POSTGRUSTQL_PORT=5433 postgrustql

# Use different data directory
POSTGRUSTQL_DATA_DIR=/mnt/data/pgr postgrustql

# Disable auto-init (for production)
POSTGRUSTQL_INITDB=false postgrustql
```

**Systemd with environment file:**

Create `/etc/postgrustsql/postgrustsql.env`:
```bash
POSTGRUSTQL_HOST=0.0.0.0
POSTGRUSTQL_PORT=5432
POSTGRUSTQL_DATA_DIR=/var/lib/postgrustsql/data
```

Edit systemd service to load it:
```bash
sudo systemctl edit postgrustsql
```

Add:
```ini
[Service]
EnvironmentFile=/etc/postgrustsql/postgrustsql.env
```

Restart:
```bash
sudo systemctl daemon-reload
sudo systemctl restart postgrustsql
```

---

### CLI Arguments

**Server** (`postgrustql`):
- Currently reads from config file and ENV only
- Future: CLI args via clap (planned for v2.3.0)

**Client** (`pgr_cli`):

```bash
pgr_cli [OPTIONS]

Options:
  -h, --host <HOST>        Server host [default: 127.0.0.1]
  -p, --port <PORT>        Server port [default: 5432]
  -U, --user <USER>        Database user [default: postgres]
  -d, --database <DB>      Database name [default: postgres]
  --help                   Print help
```

**Examples:**
```bash
# Connect to custom host/port
pgr_cli --host 192.168.1.100 --port 5433

# Short form
pgr_cli -h localhost -p 5432

# With environment variable fallback
POSTGRUSTQL_HOST=192.168.1.100 pgr_cli
```

---

## Running the Server

### Manual Start

**Foreground (for testing):**
```bash
# Use local data directory
POSTGRUSTQL_DATA_DIR=./data postgrustql

# Use config file
postgrustql
```

**Background (daemon):**
```bash
nohup postgrustql > /var/log/postgrustsql.log 2>&1 &
```

**Stop:**
```bash
killall postgrustql
# or find PID:
ps aux | grep postgrustql
kill <PID>
```

---

### Systemd Service

**Enable service:**
```bash
sudo systemctl enable postgrustsql
```

**Start/Stop/Restart:**
```bash
sudo systemctl start postgrustsql
sudo systemctl stop postgrustsql
sudo systemctl restart postgrustsql
```

**Check status:**
```bash
sudo systemctl status postgrustsql
```

**View logs:**
```bash
# Follow logs
sudo journalctl -u postgrustsql -f

# Show last 100 lines
sudo journalctl -u postgrustsql -n 100

# Show logs since boot
sudo journalctl -u postgrustsql -b
```

**Check port binding:**
```bash
sudo netstat -tulpn | grep 5432
# or:
sudo ss -tulpn | grep 5432
```

---

## Client Tools

### pgr_cli - Interactive Client

```bash
# Connect to local server
pgr_cli

# Custom host/port
pgr_cli -h 192.168.1.100 -p 5433

# Help
pgr_cli --help
```

**Inside pgr_cli:**
```sql
-- Create table
CREATE TABLE users (id SERIAL, name TEXT, age INTEGER);

-- Insert data
INSERT INTO users (name, age) VALUES ('Alice', 30);

-- Query
SELECT * FROM users WHERE age > 25;

-- Show tables
\dt

-- Quit
quit
```

**Features:**
- ✅ Command history (Up/Down arrows)
- ✅ Line editing (Ctrl+A/E, Ctrl+W, etc.)
- ✅ History file: `~/.pgr_cli_history`
- ✅ Auto-reconnect on connection loss

---

### pgr_dump - Backup Utility

```bash
# Dump to SQL (default)
pgr_dump -o backup.sql

# Dump to binary format
pgr_dump --format binary -o backup.bin

# Schema only (no data)
pgr_dump --schema-only -o schema.sql

# Data only (no DDL)
pgr_dump --data-only -o data.sql

# Help
pgr_dump --help
```

**Options:**
- `-o, --output <FILE>` - Output file (required)
- `--format <sql|binary>` - Dump format (default: sql)
- `--schema-only` - Export schema only (CREATE statements)
- `--data-only` - Export data only (INSERT statements)
- `-h, --host <HOST>` - Server host (default: 127.0.0.1)
- `-p, --port <PORT>` - Server port (default: 5432)

---

### pgr_restore - Restore Utility

```bash
# Restore from SQL dump
pgr_restore backup.sql

# Restore from binary dump
pgr_restore backup.bin

# Dry run (show SQL without executing)
pgr_restore --dry-run backup.sql

# Custom host/port
pgr_restore -h 192.168.1.100 -p 5433 backup.sql

# Help
pgr_restore --help
```

**Options:**
- `<FILE>` - Input file (auto-detects format)
- `--dry-run` - Show SQL without executing
- `-h, --host <HOST>` - Server host (default: 127.0.0.1)
- `-p, --port <PORT>` - Server port (default: 5432)

---

## Backup & Restore

### Full Backup Workflow

```bash
# 1. Create full SQL backup
pgr_dump -o /backups/pgr_$(date +%Y%m%d_%H%M%S).sql

# 2. Create compressed backup
pgr_dump -o - | gzip > /backups/pgr_$(date +%Y%m%d).sql.gz

# 3. Binary backup (faster, smaller)
pgr_dump --format binary -o /backups/pgr_$(date +%Y%m%d).bin
```

### Restore Workflow

```bash
# 1. Stop applications using the database
sudo systemctl stop myapp

# 2. Restore backup
pgr_restore /backups/pgr_20241219.sql

# 3. Or restore compressed
gunzip -c /backups/pgr_20241219.sql.gz | pgr_restore -

# 4. Restart applications
sudo systemctl start myapp
```

### Automated Backups (cron)

Create `/usr/local/bin/pgr_backup.sh`:
```bash
#!/bin/bash
BACKUP_DIR="/var/backups/postgrustsql"
DATE=$(date +%Y%m%d_%H%M%S)

mkdir -p "$BACKUP_DIR"

pgr_dump -o "$BACKUP_DIR/backup_$DATE.sql"
gzip "$BACKUP_DIR/backup_$DATE.sql"

# Keep only last 7 days
find "$BACKUP_DIR" -name "backup_*.sql.gz" -mtime +7 -delete
```

Make executable and add to cron:
```bash
sudo chmod +x /usr/local/bin/pgr_backup.sh
sudo crontab -e
```

Add line:
```cron
0 2 * * * /usr/local/bin/pgr_backup.sh
```

---

## Troubleshooting

### Server won't start

**Check if port is already in use:**
```bash
sudo netstat -tulpn | grep 5432
# or:
sudo lsof -i :5432
```

**Fix:** Kill conflicting process or use different port:
```bash
POSTGRUSTQL_PORT=5433 postgrustql
```

**Check logs:**
```bash
sudo journalctl -u postgrustsql -n 50
```

**Check data directory permissions:**
```bash
ls -ld /var/lib/postgrustsql/data
sudo chown -R postgres:postgres /var/lib/postgrustsql
```

---

### Client can't connect

**Check server is running:**
```bash
sudo systemctl status postgrustsql
ps aux | grep postgrustql
```

**Check firewall:**
```bash
sudo ufw allow 5432/tcp
# or:
sudo firewall-cmd --add-port=5432/tcp --permanent
sudo firewall-cmd --reload
```

**Test connection:**
```bash
nc -zv 127.0.0.1 5432
# or:
telnet 127.0.0.1 5432
```

---

### Performance issues

**Check disk space:**
```bash
df -h /var/lib/postgrustsql
```

**Run VACUUM:**
```bash
pgr_cli
> VACUUM;
> quit
```

**Check WAL files:**
```bash
ls -lh /var/lib/postgrustsql/data/wal/
```

**Monitor server logs:**
```bash
sudo journalctl -u postgrustsql -f
```

---

### Corruption recovery

**Stop server:**
```bash
sudo systemctl stop postgrustsql
```

**Backup corrupted data:**
```bash
sudo cp -r /var/lib/postgrustsql/data /var/lib/postgrustsql/data.backup
```

**Restore from backup:**
```bash
sudo rm -rf /var/lib/postgrustsql/data/*
pgr_restore /backups/latest.sql
```

**Restart server:**
```bash
sudo systemctl start postgrustsql
```

---

## Quick Reference

**Installation:**
```bash
# From source
git clone https://github.com/fr3ddy-fryd3/postgrust-sql.git
cd postgrust-sql
sudo make install

# Arch Linux
makepkg -si

# Debian/Ubuntu
dpkg-buildpackage -b
sudo dpkg -i ../postgrustsql_2.2.0-1_amd64.deb
```

**Configuration:**
```bash
# Edit config
sudo vim /etc/postgrustsql/postgrustsql.toml

# Use ENV
export POSTGRUSTQL_PORT=5433
```

**Service Management:**
```bash
sudo systemctl start postgrustsql
sudo systemctl stop postgrustsql
sudo systemctl status postgrustsql
sudo journalctl -u postgrustsql -f
```

**Backup/Restore:**
```bash
pgr_dump -o backup.sql
pgr_restore backup.sql
```

---

## Additional Resources

- **GitHub:** https://github.com/fr3ddy-fryd3/postgrust-sql
- **README:** See `README.md` for feature list and SQL syntax
- **ROADMAP:** See `ROADMAP.md` for planned features
- **Issues:** https://github.com/fr3ddy-fryd3/postgrust-sql/issues

---

**PostgrustSQL v2.2.0** - PostgreSQL-compatible database server written in Rust
