import os
import random
import sqlite3
from datetime import datetime, timedelta

# Configuration
DB_DIR = "./databases"
DB_PATH = os.path.join(DB_DIR, "database1.sqlite3")
TOTAL_RECORDS = 100_000
BATCH_SIZE = 10_000

# Sample data for randomization
CATEGORIES = ["Electronics", "Home & Kitchen", "Apparel", "Books", "Toys", "Health"]
REGIONS = ["North America", "Europe", "Asia", "South America", "Africa", "Oceania"]
PRODUCT_PREFIXES = ["Smart", "Pro", "Ultra", "Basic", "Eco", "Global", "Hyper"]
PRODUCT_TYPES = ["Widget", "Device", "Hub", "System", "Link", "Node", "Pack"]


def generate_random_name():
    """Generates a pseudo-realistic product name without external libs."""
    return f"{random.choice(PRODUCT_PREFIXES)} {random.choice(PRODUCT_TYPES)} {random.randint(100, 999)}"


def populate_db():
    # Ensure directory exists
    if not os.path.exists(DB_DIR):
        os.makedirs(DB_DIR)

    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()

    print(f"Initializing database at {DB_PATH}...")

    # 1. Create Schema
    cursor.execute("DROP TABLE IF EXISTS sales")
    cursor.execute("""
        CREATE TABLE sales (
            id INTEGER PRIMARY KEY,
            transaction_date TEXT,
            product_category TEXT,
            product_name TEXT,
            units_sold INTEGER,
            unit_price REAL,
            total_revenue REAL,
            region TEXT
        )
    """)

    # 2. Bulk Insertion
    print(f"Generating {TOTAL_RECORDS} records. Please wait...")

    # Use a manual transaction for speed
    cursor.execute("BEGIN TRANSACTION")

    start_date = datetime.now() - timedelta(days=365)

    try:
        for i in range(0, TOTAL_RECORDS, BATCH_SIZE):
            batch = []
            for _ in range(BATCH_SIZE):
                # Generate random values
                random_days = random.randint(0, 365)
                tx_date = (start_date + timedelta(days=random_days)).strftime(
                    "%Y-%m-%d"
                )
                category = random.choice(CATEGORIES)
                p_name = generate_random_name()
                units = random.randint(1, 15)
                price = round(random.uniform(5.0, 1200.0), 2)
                revenue = round(units * price, 2)
                region = random.choice(REGIONS)

                batch.append((tx_date, category, p_name, units, price, revenue, region))

            cursor.executemany(
                """
                INSERT INTO sales (transaction_date, product_category, product_name, units_sold, unit_price, total_revenue, region)
                VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
                batch,
            )

            print(f"Progress: {i + BATCH_SIZE}/{TOTAL_RECORDS} records inserted...")

        conn.commit()
        print("\nSuccess!")

        # Verify count
        cursor.execute("SELECT COUNT(*) FROM sales")
        print(f"Final record count in database: {cursor.fetchone()[0]}")

    except Exception as e:
        conn.rollback()
        print(f"An error occurred: {e}")
    finally:
        conn.close()


if __name__ == "__main__":
    populate_db()
