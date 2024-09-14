// File: src/main.rs
use reqwest::blocking::get;
use scraper::{Html, Selector};
use urlencoding::encode;
use serde_json::Value;

#[derive(Debug)]
struct GoodreadsBook {
    cover: String,
    title: String,
    author: String,
    date_added: String,
}

struct LibbyBook {
    cover: String,
    title: String,
    author: String,
    is_available: bool,
    is_holdable: bool,
    libby_search_url: String,
}

struct Library {
    name: String,
    libby_base_url: String,
    overdrive_base_url: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // URL of the user's to-read shelf
    // let url = "https://www.goodreads.com/review/list/44369181-travis-chambers?ref=nav_mybooks&shelf=to-read";
    // print=true here gives us a simpler webpage to parse
    // order=d sorts by descending
    // sort=date_added sorts by the order the books were added
    // TODO: get this to work. right now i always get 20
    // per_page=500 gives us 500 books at once. we could do more, but probably not necessary
    let url = "https://goodreads.com/review/list/44369181-travis-chambers?order=d&print=true&ref=nav_mybooks&shelf=to-read&sort=date_added&title=travis-chambers&per_page=50";
    // Fetch the page content
    let response = get(url)?.text()?;
    println!("Fetched page content successfully.");

    // Parse the HTML document
    let document = Html::parse_document(&response);
    println!("Parsed html successfully.");

    // Define selectors - i just looked at the HTML directly to determine these
    let book_rows_selector = Selector::parse("tr.bookalike.review").unwrap();
    let cover_selector = Selector::parse("td.field.cover img").unwrap();
    let title_selector = Selector::parse("td.field.title a").unwrap();
    let author_selector = Selector::parse("td.field.author a").unwrap();
    let date_added_selector = Selector::parse("td.field.date_added span").unwrap();

    let mut books: Vec<GoodreadsBook> = Vec::new();
    // Loop through the books in the to-read shelf
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
        let date_added_element = book_row.select(&date_added_selector).next().unwrap();
        let date_added = date_added_element.inner_html().trim().to_string();

        // Create and store the book information
        books.push(GoodreadsBook {
            cover,
            title,
            author,
            date_added,
        });
    }

    // Output the number of books
    println!("Total number of books: {}", books.len());

    // search all configured libraries concurrently for each book

    let mut libs: Vec<Library> = Vec::new();
    libs.push(Library {
        name: String::from("hawaii"),
        libby_base_url: String::from("https://libbyapp.com/library/hawaii"),
        overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/hawaii"),
    });
    // libs.push(Library { 
    //     name: String::from("utah"),
    //     libby_base_url: String::from("https://libbyapp.com/library/beehive"),
    //     overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/beehive"),
    // });
    // libs.push(Library {
    //     name: String::from("livermore"),
    //     libby_base_url: String::from("https://libbyapp.com/library/livermore"),
    //     overdrive_base_url: String::from("https://thunder.api.overdrive.com/v2/libraries/livermore"),
    // });
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

    let mut libby_books: Vec<LibbyBook> = Vec::new();
    for book in &books {
        let query = format!("{} {}", book.title, book.author);
        let url_safe_query = encode(&query);

        for library in &libs {
            let libby_search_url: String = format!("{}/search/query-{}/page-1", library.libby_base_url, url_safe_query);
            // TODO: make these formats configurable via leptos multiselect dropdown
            // let format_str: String = "format=ebook-overdrive,ebook-media-do,ebook-overdrive-provisional,audiobook-overdrive,audiobook-overdrive-provisional,magazine-overdrive".to_string();
            let format_str: String = "format=audiobook-overdrive,audiobook-overdrive-provisional".to_string();
            let overdrive_url = format!("{}/media?query=okay%20for%20now&{}&perPage=24&page=1&truncateDescription=false&x-client-id=dewey", library.overdrive_base_url, format_str);

            // Fetch the json from overdrive, then check the items array until we find a title that matches the book title

            // Fetch the page content
            let response = get(overdrive_url)?.text()?;

            // Parse the JSON document
            let json: Value = serde_json::from_str(&response).unwrap();
            let items = json["items"].as_array().unwrap();
            for item in items {
                let title: &str = item["title"].as_str().unwrap();
                let author: &str = item["firstCreatorSortName"].as_str().unwrap();
                let is_available: bool = item["isAvailable"].as_bool().unwrap();
                let is_holdable: bool = item["isHoldable"].as_bool().unwrap();
                let cover: &str = item["covers"]["cover150Wide"]["href"].as_str().unwrap();
                if title.to_lowercase() == book.title.to_lowercase() && author.to_lowercase() == book.author.to_lowercase() {
                    libby_books.push(LibbyBook {
                        cover: cover.to_string(),
                        title: title.to_string(),
                        author: author.to_string(),
                        is_available: is_available,
                        is_holdable: is_holdable,
                        libby_search_url: libby_search_url.to_string(),
                    });
                }
            }
        }
    }

    Ok(())


}
