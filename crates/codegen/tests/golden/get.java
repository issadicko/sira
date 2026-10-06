import okhttp3.Headers;
import okhttp3.MediaType;
import okhttp3.OkHttpClient;
import okhttp3.Request;
import okhttp3.RequestBody;
import okhttp3.Response;

public class Main {
    public static void main(String[] args) throws Exception {
        OkHttpClient client = new OkHttpClient();

        Request request = new Request.Builder()
            .url("http://127.0.0.1:8765/items?x=1&y=a%20b")
            .get()
            .headers(new Headers.Builder()
                .add("Accept", "application/json")
                .addUnsafeNonAscii("X-Odd", "valé \"double\" 'simple' $dollar \\anti `tick`")
                .build())
            .build();

        try (Response response = client.newCall(request).execute()) {
            System.out.println(response.code());
            System.out.println(response.body().string());
        }
    }
}
