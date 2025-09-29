FROM node:20-alpine AS build-stage

WORKDIR /comeet-notify

COPY package.json .

RUN npm install

COPY . .

RUN npm run build

FROM node:20-alpine AS prod-stage

COPY --from=build-stage /comeet-notify/dist /comeet-notify/dist
COPY --from=build-stage /comeet-notify/package.json /comeet-notify/package.json

WORKDIR /comeet-notify

RUN npm install --production \
    && npm install -g pm2

CMD ["pm2-runtime", "dist/main.js", "-i", "max"]
