# Noeracle attestation service.
#
# Builds and runs the keeper (scripts/keeper) — polls exchanges, signs
# prices, and serves them over HTTP. The publisher signing key is supplied
# at runtime via the NOERACLE_PUBLISHER_SECRET_HEX environment variable;
# set it as a Fly secret, never bake it into the image.

FROM node:22-slim

WORKDIR /app

# Install dependencies (the keeper uses @noble/curves and dotenv).
COPY scripts/package.json scripts/package-lock.json ./
RUN npm ci --omit=dev

# Keeper source.
COPY scripts/keeper ./keeper

EXPOSE 8080
CMD ["node", "keeper/index.mjs"]
