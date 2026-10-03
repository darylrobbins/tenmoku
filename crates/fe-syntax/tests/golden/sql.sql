-- A small library schema, used as a golden highlighting sample.

CREATE TABLE authors (
    id INTEGER PRIMARY KEY,
    name VARCHAR(120) NOT NULL,
    born DATE,
    active BOOLEAN DEFAULT TRUE
);

CREATE TABLE books (
    id INTEGER PRIMARY KEY,
    author_id INTEGER NOT NULL REFERENCES authors (id),
    title TEXT NOT NULL,
    price DECIMAL(8, 2) DEFAULT 0.00,
    published TIMESTAMP
);

CREATE INDEX books_by_author ON books (author_id);

/* Seed a few rows. */
INSERT INTO authors (id, name, born, active)
VALUES
    (1, 'Ursula Le Guin', '1929-10-21', FALSE),
    (2, 'Ted Chiang', '1967-10-20', TRUE);

INSERT INTO books (id, author_id, title, price)
VALUES (10, 1, 'The Dispossessed', 12.50), (11, 2, 'Exhalation', 15);

UPDATE books
SET price = price * 1.1
WHERE published IS NULL AND price < 20;

DELETE FROM books WHERE title LIKE '%draft%';

-- Authors with their book count and average price.
SELECT
    a.name AS author,
    COUNT(b.id) AS books,
    COALESCE(AVG(b.price), 0) AS average_price
FROM authors AS a
LEFT JOIN books AS b ON b.author_id = a.id
WHERE a.active = TRUE OR a.born < '1950-01-01'
GROUP BY a.name
HAVING COUNT(b.id) > 0
ORDER BY books DESC, author ASC
LIMIT 10;

WITH recent AS (
    SELECT id, title FROM books WHERE published >= '2020-01-01'
)
SELECT title,
    CASE
        WHEN LENGTH(title) > 12 THEN 'long'
        ELSE 'short'
    END AS size
FROM recent;

ALTER TABLE books ADD COLUMN isbn VARCHAR(13);

DROP TABLE IF EXISTS archive;
