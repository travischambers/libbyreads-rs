# libbyreads-rs
Find which of your libraries have digital books available from your Goodreads want-to-read shelf. 
A rust port (and upgrade!) of the original [libbyreads](https://github.com/travischambers/libbyreads).

For users with long to-read shelves in Goodreads, finding the next library book to checkout can be daunting.
This is exacerbated when users have access to several libraries in Libby.

The intention is to provide a simple web app where:
1. a User inputs their Goodreads info (without requiring any credentials)
2. libbyreads reads all the books from their to-read (or any!) shelf.
3. a User inputs which Libby libraries they have access to
4. libbyreads checks all libraries for each book on their shelf and reports which books are available now, can be placed on hold, or are not available at all.

# Getting Started

Create a `.env` file in the repo root. In this file, define three env vars:
- HONEYCOMB_API_KEY=<your-honeycomb-api-key>
- HONEYCOMB_DATASET=libbyreads
- HONEYCOMB_LOG_API_ENDPOINT=https://api.honeycomb.io/v1/logs