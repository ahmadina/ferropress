# Reference application image for the lab oracle. Every input is pinned in
# contracts/reference-manifest.toml and passed in by `cargo xtask lab up`.
ARG LAB_PHP_IMAGE
FROM ${LAB_PHP_IMAGE}

ARG LAB_PLATFORM_URL
ARG LAB_PLATFORM_SHA256

ARG LAB_FAKETIME_VERSION=0.9.10+2024-06-05+gba9ed5b2-0.6

RUN docker-php-ext-install -j"$(nproc)" mysqli \
    && a2enmod rewrite

# Every process in the container sees the deterministic wall-clock time set in FAKETIME (compose.yaml),
# so installs and requests are repeatable; monotonic clocks stay real so timeouts keep working.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends "libfaketime=${LAB_FAKETIME_VERSION}" \
    && rm -rf /var/lib/apt/lists/* \
    && ln -s "$(dpkg -L libfaketime | grep '/libfaketime\.so\.1$')" /usr/local/lib/libfaketime.so.1
ENV LD_PRELOAD=/usr/local/lib/libfaketime.so.1 \
    FAKETIME_DONT_FAKE_MONOTONIC=1

ADD --checksum=sha256:${LAB_PLATFORM_SHA256} ${LAB_PLATFORM_URL} /tmp/platform.tar.gz

# Database settings are read from the container environment at runtime, so the image holds no credentials.
# The secret keys are fixed lab-only values (so no random salts are generated), and the lab settings and
# instrumentation in /opt/ferropress are required where the config accepts custom values.
RUN tar -xzf /tmp/platform.tar.gz -C /var/www/html --strip-components=1 \
    && rm /tmp/platform.tar.gz \
    && cd /var/www/html \
    && sed -e "s/'database_name_here'/getenv('LAB_DB_NAME')/" \
           -e "s/'username_here'/getenv('LAB_DB_USER')/" \
           -e "s/'password_here'/getenv('LAB_DB_PASSWORD')/" \
           -e "s/'localhost'/getenv('LAB_DB_HOST')/" \
           -e "/Add any custom values between this line/a require '/opt/ferropress/config.php';" \
           -E -e "s/define\( '([A-Z_]+)',( +)'put your unique phrase here' \);/define( '\1',\2'ferropress-lab-only-\1' );/" \
           wp-config-sample.php > wp-config.php \
    && grep -q "getenv('LAB_DB_HOST')" wp-config.php \
    && ! grep -q "put your unique phrase here" wp-config.php \
    && grep -q "^require '/opt/ferropress/config.php';" wp-config.php \
    && chown -R www-data:www-data /var/www/html

COPY config.php instrument.php install-site.php inventory.php serialize-oracle.php value-oracle.php lexer-oracle.php lexer-corpus.php parser-oracle.php parser-corpus.php /opt/ferropress/
COPY sites.conf /etc/apache2/sites-available/000-default.conf
# Our own behavior-free plugins, so traces show where must-use and regular plugins load.
COPY --chown=www-data:www-data fixtures/ /var/www/html/wp-content/
