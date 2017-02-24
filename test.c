#include <unistd.h>
#include "aml_player.h"

int main() {
	void* p = aml_video_player_create();
	if (p == NULL) {
		return -2;
	}
	if (aml_video_player_load(p, "~/bigbuckbunny-hevc-4K.mp4") != 0){
		return -1;
	};
	aml_video_player_resize(p, 1280, 720);
	aml_video_player_play(p);
	sleep(3);
	aml_video_player_wait_until_end(p);
	aml_video_player_destroy(p);
}
