import okhttp3.MediaType;
import okhttp3.MultipartBody;
import okhttp3.OkHttpClient;
import okhttp3.Request;
import okhttp3.RequestBody;
import okhttp3.Response;
import java.io.File;

public class Main {
    public static void main(String[] args) throws Exception {
        OkHttpClient client = new OkHttpClient();

        RequestBody body = new MultipartBody.Builder()
            .setType(MultipartBody.FORM)
            .addFormDataPart("title", "Photo; \"cover\"")
            .addFormDataPart("note", null, RequestBody.create("typed", MediaType.parse("text/plain")))
            .addFormDataPart("avatar", "a.png", RequestBody.create(new File("files/a.png"), MediaType.parse("image/png")))
            .addFormDataPart("doc", "b.txt", RequestBody.create(new File("files/b.txt"), null))
            .build();

        Request request = new Request.Builder()
            .url("http://127.0.0.1:8765/upload")
            .method("POST", body)
            .addHeader("X-Trace", "1")
            .build();

        try (Response response = client.newCall(request).execute()) {
            System.out.println(response.code());
            System.out.println(response.body().string());
        }
    }
}
