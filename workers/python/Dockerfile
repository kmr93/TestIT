# Pinned isolated Python worker container for TestIT
FROM python:3.11-slim

WORKDIR /app

# Install minimal OS dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Pinned Python requirements
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Create non-root runner user
RUN useradd -m -u 10001 -s /bin/sh testit

# Copy worker application code
COPY . /app
RUN chown -R testit:testit /app

USER testit

ENTRYPOINT ["python", "/app/main.py"]
