# ihaCDN (Rust)

![Status](https://img.shields.io/uptimerobot/status/m784617086-4e68d7e9dd7670f5c03bc09b?label=Status&style=for-the-badge) ![Uptime (7 days)](https://img.shields.io/uptimerobot/ratio/7/m784617086-4e68d7e9dd7670f5c03bc09b?style=for-the-badge)

A simple file hosting using axum.<br>
This is a port of TypeScript version with Express.js framework right here: [ihacdn-server-ts](https://github.com/ihateani-me/ihacdn-server-ts)<br>
**Currently running on: [https://p.ihateani.me/](https://p.ihateani.me/)**

## Feature
- Image/Files/Text support with blacklisting.
- Auto determining if it's text or files without user defining itself.
- Filesize limit support. **[Can be disabled.]**
- [Customizable](#configuration).
- Code highlighting support via **Shiki.js**
- Shortlink generation support
- You don't need the extension to access your files/code
- [File retention](#file-retention) support<br>
Formula: `min_days + (-max_days + min_days) * (file_size / filesize_limit - 1) ** 5`
- Discord Webhook Notification Support

## Using the filehosting.
There's 2 POST endpoint:
- `/upload` for image/files/text
- `/short` for shortening link.

To upload, you need to provide file with the name `file`.<br>
To shorten url, you need to use form data with `url` as the key.

**Example with curl**:<br>
Uploading files:<br>
```bash
curl -X POST -F "file=@yourfile.png" https://p.ihateani.me/upload
```

Shortening link:<br>
```bash
curl -X POST -F "url=http://your.long/ass/url/that/you/want/to/shorten" https://p.ihateani.me/short
```

Or you could use [ShareX](https://getsharex.com/) and import the provided [sxcu](https://github.com/ihateani-me/ihacdn-server/tree/master/sharex) files.

## Setup
What you need:
- Rust 1.85.0
- Redis 7

To install and run:
1. Download Rust and other build requirements
2. Clone this repository
3. Rename `config.json.example` to `config.json`
4. Go to [Configuration](#configuration) to config stuff first.
5. Run `cargo build --locked --profile release`
6. Run `./target/release/ihacdn` or `.\target\release\ihacdn.exe` on Windows
8. By default your server will be hosted at https://127.0.0.1:6969

## Configuration
Configure this program by opening `config.json`<br>
You will see a lot of stuff that you could change.

```jsonc
{
    "hostname": "localhost", // Hostname that will be used.
    "host": "127.0.0.1", // The host where the program will be running
    "port": 6969, // The port where the program will be running.
    "https_mode": false, // Enable HTTPS Mode or not
    "upload_path": "./", // The saved uploads
    "admin_password": "pleasechangethis", // Password for Admin
    "filename_length": 8, // Randomized password length
    "redisdb": {
        "host": "127.0.0.1", // Redis Host
        "port": 6379, // Redis Port
        "password": null // Redis password, leave at null if there's none
    },
    "notifier": {
        "enable": false, // This will enable the notifier for a new upload or short
        "discord_webhook": null // discord webhook URL
    },
    "file_retention": {
        "enable": false, // This will enable file retention before being deleted from server
        "min_age": 30, // Minimum age in days before deletion
        "max_age": 180 // Maximum age in days before deletion
    },
    "storage": {
        "filesize_limit": 512, // Filesize limit for normal user (in kb), leave at null if you don't want any limit
        "admin_filesize_limit": null // Filesize limit for admin (in kb), leave at null if you don't want any limit
    },
    "blocklist": { // Block certain type of file
        "extension": [
            "exe",
            "sh",
            "msi",
            "bat",
            "dll",
            "com"
        ],
        "content_type": [
            "text/x-sh",
            "text/x-msdos-batch",
            "application/x-dosexec",
            "application/x-msdownload",
            "application/vnd.microsoft.portable-executable",
            "application/x-msi",
            "application/x-msdos-program",
            "application/x-sh"
        ]
    }
}
```

That's the default settings, you can adjust it what you want.

Explanation:
- **hostname**: are your website domain.
- **https_mode**: is your website gonna run on https or not.
- **upload_path**: where to put your uploads path, recommended to leave it just like that.
- **admin_password**: admin password, please modify this.
- **filename_length**: the randomized filename length.
- **redis**: The redis:// database configuration URL
- **notifier**
  - **enable**: Enable notifier that will notify for a new upload or link shorten
  - **discord_webhook**: if you want to use discord webhook, add your webhook url here or leave it to `null` if you don't need it.
- **file_retention**
  - **enable**: Enable file retention that basically will time the file before deletion
  - **min_age**: Minimum age of file being saved in server (in days)
  - **max_age**: Minimum age of file being saved in server (in days)
- **storage**
  - **filesize_limit**: upload size limit (in kilobytes) for normal user. (can be set to `None` for no limit.)
  - **admin_filesize_limit**: upload size limit (in kilobytes) for someone using admin password (can be set to `None` for no limit.)
- **blocklist**
  - **extension**: Blocked extension, this will not allow anything with this extension.
  - **content_type**: Blocked content-type, this will not allow any file with this content-type.

## File Retention
[To be written.]

## Deployment

If you're using Reverse Proxy like Nginx, it's recommended to set `client_max_body_size` to make sure you can upload large files.<br>
You can either put it on `http` block on `/etc/nginx/nginx.conf` or the `server` block on `sites-available` conf.

Example:
```py
http {
    ...
    client_max_body_size 1024M; # This will set the limit to 1 GiB.
    ...
}
```

## External acknowledgements
This project uses the following libraries for Pastebin:
- [`shiki`](https://shiki.style/) - for syntax highlighting
- [`IBM Mono Plex`](https://fonts.google.com/specimen/IBM+Plex+Mono) - for font

*This project is licensed with MIT License*