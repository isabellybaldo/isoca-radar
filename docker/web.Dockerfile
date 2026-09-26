# syntax=docker/dockerfile:1
# Build context: repository root.  docker compose -f compose.yml -f compose.web.yml --profile web build web
FROM node:22-alpine AS build
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

# Unprivileged nginx (uid 101, port 8080): runs with a read-only root filesystem plus a /tmp tmpfs.
FROM nginxinc/nginx-unprivileged:1.29-alpine
COPY docker/web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /web/dist /usr/share/nginx/html
USER 101
EXPOSE 8080
