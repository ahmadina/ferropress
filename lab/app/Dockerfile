# Reference application image for the lab oracle. Every input is pinned in
# contracts/reference-manifest.toml and passed in by `cargo xtask lab up`.
ARG LAB_PHP_IMAGE
FROM ${LAB_PHP_IMAGE}

ARG LAB_PLATFORM_URL
ARG LAB_PLATFORM_SHA256

RUN docker-php-ext-install -j"$(nproc)" mysqli \
    && a2enmod rewrite

ADD --checksum=sha256:${LAB_PLATFORM_SHA256} ${LAB_PLATFORM_URL} /tmp/platform.tar.gz

# Database settings are read from the container environment at runtime, so the image holds no credentials.
RUN tar -xzf /tmp/platform.tar.gz -C /var/www/html --strip-components=1 \
    && rm /tmp/platform.tar.gz \
    && cd /var/www/html \
    && sed -e "s/'database_name_here'/getenv('LAB_DB_NAME')/" \
           -e "s/'username_here'/getenv('LAB_DB_USER')/" \
           -e "s/'password_here'/getenv('LAB_DB_PASSWORD')/" \
           -e "s/'localhost'/getenv('LAB_DB_HOST')/" \
           wp-config-sample.php > wp-config.php \
    && grep -q "getenv('LAB_DB_HOST')" wp-config.php \
    && chown -R www-data:www-data /var/www/html
