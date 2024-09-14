// File: src/main.rs
use std::sync::{Arc, Mutex};
use tokio::task;

use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use urlencoding::encode;

#[derive(Debug)]
struct GoodreadsBook {
    cover: String,
    title: String,
    author: String,
    date_added: String,
}
#[derive(Debug)]
#[derive(Clone)]
struct LibbyLibraryBook {
    cover: String,
    title: String,
    author: String,
    is_available: bool,
    is_holdable: bool,
    // we don't track is_owned directly, because we can infer it from is_available and is_holdable
    libby_search_url: String,
}
#[derive(Debug)]
struct LibbyBook {
    cover: String,
    title: String,
    author: String,
    is_available: bool,
    is_holdable: bool,
    // we don't track is_owned directly, because we can infer it from is_available and is_holdable
    libby_search_url: String,
    library_books: Vec<LibbyLibraryBook>,
}
#[derive(Debug)]
struct Library {
    name: String,
    libby_base_url: String,
    overdrive_base_url: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let books = Arc::new(Mutex::new(Vec::new()));

    // URL of the user's to-read shelf
    // print=true here gives us a simpler webpage to parse
    // order=d sorts by descending
    // sort=date_added sorts by the order the books were added
    // TODO: get per_page to work. right now i always get 20
    // per_page=500 gives us 500 books at once. we could do more, but probably not necessary
    // TODO: make the shelf configurable via leptos multiselect dropdown
    let url = "https://goodreads.com/review/list/44369181-travis-chambers?order=d&print=true&ref=nav_mybooks&shelf=to-read&sort=date_added";

    // Fetch the first page content to determine the total number of pages
    let client = Client::new();
    let response = client.get(url).send().await?.text().await?;
    println!("Fetched page content successfully.");

    // Parse the HTML document
    let document = Html::parse_document(&response);
    println!("Parsed html successfully.");

    // get the total number of pages
    let pagination_selector = Selector::parse("#reviewPagination a").unwrap();

    // Find the highest number in the pagination links
    let mut last_page = document
        .select(&pagination_selector)
        .filter_map(|element| element.text().collect::<String>().parse::<u32>().ok())
        .max()
        .unwrap_or(1); // If there are no pagination links, there is only one page
    println!("Total pages: {}", last_page);
    // TODO: remove this when i asyncify library/book searches
    if last_page > 2 {
        // only fetch the first 2 pages for now
        last_page = 2;
        println!("Only fetching the first 2 pages for now.");
    }

    // Create async tasks for each page
    let mut tasks = vec![];
    for page_number in 1..=last_page {
        let books = Arc::clone(&books); // Clone the Arc for each task
        let client = client.clone(); // Clone the client to reuse it
        let page_url = format!("{}&page={}", url, page_number);

        // Spawn a new async task to fetch and parse the page
        let task = task::spawn(async move {
            if let Ok(response) = client.get(&page_url).send().await {
                if let Ok(text) = response.text().await {
                    let document = Html::parse_document(&text);
                    // Define selectors - i just looked at the HTML directly to determine these
                    let book_rows_selector = Selector::parse("tr.bookalike.review").unwrap();
                    let cover_selector = Selector::parse("td.field.cover img").unwrap();
                    let title_selector = Selector::parse("td.field.title a").unwrap();
                    let author_selector = Selector::parse("td.field.author a").unwrap();
                    let date_added_selector = Selector::parse("td.field.date_added span").unwrap();

                    // Loop through each book row
                    for book_row in document.select(&book_rows_selector) {
                        // Get cover image
                        let cover_element = book_row.select(&cover_selector).next().unwrap();
                        let cover = cover_element.value().attr("src").unwrap().to_string();

                        // Get title
                        let title_element = book_row.select(&title_selector).next().unwrap();
                        let title = title_element.inner_html().trim().to_string();

                        // Get author
                        let author_element = book_row.select(&author_selector).next().unwrap();
                        let author = author_element.inner_html().trim().to_string();

                        // Get date added
                        let date_added_element =
                            book_row.select(&date_added_selector).next().unwrap();
                        let date_added = date_added_element.inner_html().trim().to_string();

                        // Create a book struct
                        let book = GoodreadsBook {
                            cover,
                            title,
                            author,
                            date_added,
                        };

                        // Add the book to the shared vector
                        let mut books_guard = books.lock().unwrap();
                        books_guard.push(book);
                    }
                }
            }
        });
        tasks.push(task);
    }

    // Await all tasks
    for task in tasks {
        task.await?;
    }

    // Print all the books
    let books = books.lock().unwrap();
    for book in books.iter() {
        println!("{:?}", book);
    }

    println!("Total number of books: {}", books.len());

    // search all configured libraries concurrently for each book
    let mut libs = Vec::new();
    libs.push(Library {
        name: String::from("hawaii"),
        libby_base_url: String::from("https://libbyapp.com/library/hawaii"),
        overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/hawaii"),
    });
    libs.push(Library {
        name: String::from("utah"),
        libby_base_url: String::from("https://libbyapp.com/library/beehive"),
        overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/beehive"),
    });
    libs.push(Library {
        name: String::from("livermore"),
        libby_base_url: String::from("https://libbyapp.com/library/livermore"),
        overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/livermore"),
    });
    // libs.push(Library {
    //     name: String::from("edmonton"),
    //     libby_base_url: String::from("https://libbyapp.com/library/epl"),
    //     overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/epl"),
    // });
    // libs.push(Library {
    //     name: String::from("georgia"),
    //     libby_base_url: String::from("https://libbyapp.com/library/gadd"),
    //     overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/gadd"),
    // });

    let mut libby_books = Vec::new();
    for book in books.iter() {
        let query = format!("{} {}", book.title, book.author);
        let url_safe_query = encode(&query);
        let mut libby_library_books = Vec::new();

        for library in &libs {
            let libby_search_url: String = format!(
                "{}/search/query-{}/page-1",
                library.libby_base_url, url_safe_query
            );
            // TODO: make these formats configurable via leptos multiselect dropdown
            // let format_str: String = "format=ebook-overdrive,ebook-media-do,ebook-overdrive-provisional,audiobook-overdrive,audiobook-overdrive-provisional,magazine-overdrive".to_string();
            let format_str: String =
            "format=audiobook-overdrive,audiobook-overdrive-provisional".to_string();
            let overdrive_url = format!(
                "{}/media?query={}&{}&perPage=24&page=1&truncateDescription=false&x-client-id=dewey",
                library.overdrive_base_url, 
                url_safe_query,
                format_str,
            );
            println!("Searching for book: {} by {} at {}", book.title, book.author, library.name);

            // Fetch the json from overdrive, then check the items array until we find a title that matches the book title

            // Fetch the page content
            let response = client.get(overdrive_url.clone()).send().await?.text().await?;

            // Parse the JSON document
            let json: Value = serde_json::from_str(&response).unwrap();
            let items = json["items"].as_array().unwrap();
            let mut book_found_at_library = false;
            for item in items {
                let title: &str = item["title"].as_str().unwrap();
                let author: &str = item["firstCreatorSortName"].as_str().unwrap();
                let is_available: bool = item["isAvailable"].as_bool().unwrap();
                let is_holdable: bool = item["isHoldable"].as_bool().unwrap();
                let cover: &str = item["covers"]["cover150Wide"]["href"].as_str().unwrap();
                println!("Found book in overdrive: {} by {} at {}", title, author, library.name);
                println!();

                if title.to_lowercase() == book.title.to_lowercase()
                    && author.to_lowercase() == book.author.to_lowercase()
                {
                    let libby_library_book = LibbyLibraryBook {
                        cover: cover.to_string(),
                        title: title.to_string(),
                        author: author.to_string(),
                        is_available: is_available,
                        is_holdable: is_holdable,
                        libby_search_url: libby_search_url.to_string(),
                    };
                    libby_library_books.push(libby_library_book);
                    book_found_at_library = true;
                    break;
                }
            }
            if !book_found_at_library {
                println!("Did not find book {} in libby at {}.", book.title, library.name);
                println!("{}", libby_search_url);
                println!("{}", overdrive_url);
                println!();
                libby_library_books.push(LibbyLibraryBook {
                    cover: "".to_string(),
                    title: book.title.to_string(),
                    author: book.author.to_string(),
                    is_available: false,
                    is_holdable: false,
                    libby_search_url: libby_search_url.to_string(),
                })
            }
        }
        // find a library where `is_available` is true
        // if not found, find a library where `is_holdable` is true
        let mut is_available = false;
        let mut is_holdable = false;
        // initialize to the libby_search_url of the first library
        let mut libby_search_url = &libby_library_books[0].libby_search_url;
        for libby_library_book in libby_library_books.iter() {
            if libby_library_book.is_available {
                is_available = true;
                libby_search_url = &libby_library_book.libby_search_url;
                break;
            }
            if is_holdable == false && libby_library_book.is_holdable {
                is_holdable = true;
                libby_search_url = &libby_library_book.libby_search_url;
            }
        }
        let libby_book = LibbyBook {
            cover: book.cover.to_string(),
            title: book.title.to_string(),
            author: book.author.to_string(),
            is_available: is_available,
            is_holdable: is_holdable,
            libby_search_url: libby_search_url.to_string(),
            library_books: libby_library_books.clone(),
        };
        libby_books.push(libby_book);
    }

    // print a count of is_available, is_holdable, and not_owned
    let mut available_count = 0;
    let mut holdable_count = 0;
    let mut not_owned_count = 0;
    for libby_book in libby_books.iter() {
        if libby_book.is_available {
            available_count += 1;
        }
        else if libby_book.is_holdable {
            holdable_count += 1;
        }
        else {
            not_owned_count += 1;
        }
    }
    // print available books
    println!("Available books: {}", available_count);
    for libby_book in libby_books.iter() {
        if libby_book.is_available {
            println!("{:?}", libby_book);
        }
    }
    println!();
    println!();
    // print holdable books
    println!("Holdable books: {}", holdable_count);
    for libby_book in libby_books.iter() {
        if libby_book.is_holdable {
            println!("{:?}", libby_book);
        }
    }
    println!();
    println!();
    // print summary
    println!(
        "Available: {}, Holdable: {}, Not Owned: {}, Total: {}",
        available_count, holdable_count, not_owned_count, libby_books.len()
    );
    Ok(())
}
