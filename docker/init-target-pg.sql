-- Target Postgres initialization: sample schema, data, and sqlgate roles.
-- Applied automatically by docker-compose on first start.

-- Roles for sqlgate's preview and execute engines.
-- sqlgate_preview: read-only — used for query previews (SELECT only).
-- sqlgate_execute: read-write — used to run approved DML/DDL.
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = 'sqlgate_preview') THEN
        CREATE ROLE sqlgate_preview WITH LOGIN PASSWORD 'preview' NOSUPERUSER NOCREATEDB NOCREATEROLE;
    END IF;
    IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = 'sqlgate_execute') THEN
        CREATE ROLE sqlgate_execute WITH LOGIN PASSWORD 'execute' NOSUPERUSER NOCREATEDB NOCREATEROLE;
    END IF;
END
$$;

-- Sample tables
CREATE TABLE IF NOT EXISTS users (
    id    SERIAL PRIMARY KEY,
    name  TEXT NOT NULL,
    email TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS orders (
    id          SERIAL PRIMARY KEY,
    user_id     INT NOT NULL REFERENCES users(id),
    amount      NUMERIC(10, 2) NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'shipped', 'delivered', 'cancelled')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS products (
    id    SERIAL PRIMARY KEY,
    name  TEXT NOT NULL,
    price NUMERIC(10, 2) NOT NULL,
    stock INT NOT NULL DEFAULT 0
);

-- Sample data
INSERT INTO users (name, email) VALUES
    ('Alice Johnson', 'alice@example.com'),
    ('Bob Smith',     'bob@example.com'),
    ('Carol Davis',   'carol@example.com')
ON CONFLICT (email) DO NOTHING;

INSERT INTO orders (user_id, amount, status) VALUES
    (1, 99.99, 'shipped'),
    (1, 49.50, 'pending'),
    (2, 200.00, 'delivered'),
    (3, 15.75, 'cancelled')
ON CONFLICT DO NOTHING;

INSERT INTO products (name, price, stock) VALUES
    ('Widget',      9.99, 100),
    ('Gadget',     24.99,  50),
    ('Thingamajig', 4.50, 200)
ON CONFLICT DO NOTHING;

-- Grant preview role read-only access to all tables in public schema.
-- New tables created after init also get read access via ALTER DEFAULT PRIVILEGES.
GRANT USAGE ON SCHEMA public TO sqlgate_preview;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO sqlgate_preview;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO sqlgate_preview;

-- Grant execute role full DML access.
GRANT USAGE ON SCHEMA public TO sqlgate_execute;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO sqlgate_execute;
GRANT USAGE ON ALL SEQUENCES IN SCHEMA public TO sqlgate_execute;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO sqlgate_execute;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT USAGE ON SEQUENCES TO sqlgate_execute;
