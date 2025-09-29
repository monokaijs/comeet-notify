FROM node:20-alpine AS build-stage

WORKDIR /comeet-notify

COPY package.json .

RUN npm install

COPY . .

RUN npm run build

FROM node:20-alpine AS prod-stage

COPY --from=build-stage /sms-otp/dist /sms-otp/dist
COPY --from=build-stage /sms-otp/package.json /sms-otp/package.json

WORKDIR /sms-otp

RUN npm install --production \
    && npm install -g pm2

CMD ["pm2-runtime", "dist/main.js", "-i", "max"]
