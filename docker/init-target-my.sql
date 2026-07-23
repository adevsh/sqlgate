-- Target MySQL initialization: sample schema, data, and sqlgate users.
-- Applied automatically by docker-compose on first start.
-- MySQL auto-runs .sql files in /docker-entrypoint-initdb.d/.

-- Users for sqlgate's preview and execute engines.
-- Using mysql_native_password for broad client compatibility.
CREATE USER IF NOT EXISTS 'sqlgate_preview'@'%' IDENTIFIED BY 'preview';
CREATE USER IF NOT EXISTS 'sqlgate_execute'@'%' IDENTIFIED BY 'execute';

-- Sample tables
CREATE TABLE IF NOT EXISTS users (
    id         INT AUTO_INCREMENT PRIMARY KEY,
    name       VARCHAR(255) NOT NULL,
    email      VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB;

CREATE TABLE IF NOT EXISTS orders (
    id         INT AUTO_INCREMENT PRIMARY KEY,
    user_id    INT NOT NULL,
    amount     DECIMAL(10, 2) NOT NULL,
    status     ENUM('pending', 'shipped', 'delivered', 'cancelled') NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id)
) ENGINE=InnoDB;

CREATE TABLE IF NOT EXISTS products (
    id    INT AUTO_INCREMENT PRIMARY KEY,
    name  VARCHAR(255) NOT NULL,
    price DECIMAL(10, 2) NOT NULL,
    stock INT NOT NULL DEFAULT 0
) ENGINE=InnoDB;

-- Sample data (INSERT IGNORE to be idempotent on container restart)
INSERT IGNORE INTO users (name, email) VALUES
    ('Alice Johnson', 'alice@example.com'),
    ('Bob Smith',     'bob@example.com'),
    ('Carol Davis',   'carol@example.com');

INSERT IGNORE INTO orders (user_id, amount, status) VALUES
    (1, 99.99, 'shipped'),
    (1, 49.50, 'pending'),
    (2, 200.00, 'delivered'),
    (3, 15.75, 'cancelled');

INSERT IGNORE INTO products (name, price, stock) VALUES
    ('Widget',      9.99, 100),
    ('Gadget',     24.99,  50),
    ('Thingamajig', 4.50, 200);

-- Grant preview user read-only access.
GRANT SELECT ON sample_my.* TO 'sqlgate_preview'@'%';

-- Grant execute user full DML access.
GRANT SELECT, INSERT, UPDATE, DELETE ON sample_my.* TO 'sqlgate_execute'@'%';

FLUSH PRIVILEGES;
