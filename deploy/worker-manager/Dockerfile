FROM python:3.11-slim

WORKDIR /app

COPY workers/python/requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir -r /tmp/requirements.txt \
    && useradd --uid 10001 --create-home --shell /usr/sbin/nologin testit

COPY workers/python/ /app/
COPY deploy/worker-manager/manager.py /manager.py

EXPOSE 8081
ENTRYPOINT ["python", "/manager.py"]
